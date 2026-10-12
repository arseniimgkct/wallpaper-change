//! Склейка приложения: общее состояние, цикл сообщений и окна.

use std::cell::RefCell;

use image::DynamicImage;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, DispatchMessageW, GetMessageW, PostQuitMessage, ShowWindow, TranslateMessage, MSG,
    SW_SHOW, WM_QUIT,
};

use crate::engine::OverlayEngine;
use crate::program::{self};
use crate::reverter;
use crate::ui::main_form;
use crate::ui::{dialogs, paint};

/// Состояние приложения. Один поток, одна копия.
pub struct App {
    /// Оверлей и значок в трее живут всё время работы приложения.
    pub engine: OverlayEngine,
    /// Окно утилиты, если оно открыто. Состояние окна лежит в самом окне.
    pub main: Option<HWND>,
    /// Скрытое окно, принимающее таймеры и значок в трее.
    pub hidden_host: Option<HWND>,
    /// Картинка, которую в данный момент показывает оверлей.
    pub image: Option<DynamicImage>,
    /// Приложение завершается по-настоящему, а не прячется в трей.
    pub really_exit: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Готовность состояния и окно трея хранятся отдельно от `APP`: их читают
/// обработчики окон, а `APP` в этот момент может быть уже занят — вызовы
/// `RedrawWindow` и `CreateWindowExW` досылают сообщения синхронно.
static APP_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static HIDDEN_HOST: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

thread_local! {
    /// Занят ли сейчас `APP`. `RedrawWindow` и перезапуск Проводника досылают
    /// сообщения синхронно, поэтому таймер или значок трея могут прийти в тот
    /// момент, когда состояние уже одолжили. Заходить второй раз нельзя:
    /// повторный заём `RefCell` при `panic = "abort"` убивает процесс.
    static APP_BUSY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Занято ли состояние прямо сейчас.
pub fn is_busy() -> bool {
    APP_BUSY.with(std::cell::Cell::get)
}

/// Есть ли состояние приложения.
pub fn is_ready() -> bool {
    APP_READY.load(std::sync::atomic::Ordering::Relaxed)
}

fn set_ready(ready: bool) {
    APP_READY.store(ready, std::sync::atomic::Ordering::Relaxed);
}

/// Запоминает скрытое окно трея, чтобы опознавать его без обращения к `APP`.
pub fn set_hidden_host(hwnd: HWND) {
    HIDDEN_HOST.store(hwnd.0 as isize, std::sync::atomic::Ordering::Relaxed);
}

/// Является ли окно скрытым хостом для режима «только трей».
pub fn window_is_hidden_host(hwnd: HWND) -> bool {
    if hwnd.0.is_null() {
        return false;
    }
    HIDDEN_HOST.load(std::sync::atomic::Ordering::Relaxed) == hwnd.0 as isize
}

/// Выполняет действие над состоянием приложения.
pub fn with_app<R>(action: impl FnOnce(&mut App) -> R) -> R {
    assert!(!is_busy(), "состояние приложения уже одолжено");

    APP_BUSY.with(|busy| busy.set(true));
    let result = APP.with(|app| {
        let mut app = app.borrow_mut();
        let app = app.as_mut().expect("состояние приложения ещё не создано");
        action(app)
    });
    APP_BUSY.with(|busy| busy.set(false));
    result
}

/// Действие над состоянием, если оно уже есть и ещё не одолжено.
pub fn try_with_app<R>(action: impl FnOnce(&mut App) -> R, fallback: R) -> R {
    if is_busy() {
        return fallback;
    }

    APP_BUSY.with(|busy| busy.set(true));
    let result = APP.with(|app| {
        let mut app = app.borrow_mut();
        match app.as_mut() {
            Some(app) => action(app),
            None => fallback,
        }
    });
    APP_BUSY.with(|busy| busy.set(false));
    result
}

/// Показывает окно утилиты из трея.
pub fn show_main_window() {
    let Some(hwnd) = try_with_app(|app| app.main, None) else {
        return;
    };

    if let Some(state) = main_form::state(hwnd) {
        state.show();
    }

    program::show_window(hwnd);
}

/// Завершает приложение из трея.
pub fn exit_application() {
    with_app(|app| {
        app.really_exit = true;
        app.engine.remove();
    });

    close_current_window();
}

/// Откат из трея: снимает всё и закрывает приложение.
pub fn revert_from_tray() -> bool {
    // Оверлей снимаем до отката: `revert_all` перезапускает Проводник, и он не
    // должен успевать перерисовать нашу картинку поверх системных обоев.
    with_app(|app| app.engine.remove());

    let report = reverter::revert_all(None, false);
    program::log(format!("откат из трея: {report}"));

    with_app(|app| {
        app.really_exit = true;
    });

    close_current_window();
    true
}

/// Статус в окне утилиты.
pub fn set_status(message: &str) {
    let hwnd = try_with_app(|app| app.main, None);
    if let Some(state) = hwnd.and_then(main_form::state) {
        state.set_status(message);
    }
}

/// Значение `GWLP_USERDATA` окна.
pub fn window_long(hwnd: HWND) -> isize {
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
        )
    }
}

/// Запускает приложение в нужном режиме и крутит цикл сообщений.
pub fn run(command: program::Command) {
    match command {
        // Откат без окна: сделали работу и вышли.
        program::Command::Revert { restore_light } => {
            let report = reverter::revert_all(None, restore_light);
            program::log(format!("откат из командной строки: {report}"));
        }

        // Автозапуск: только оверлей и трей, окна утилиты нет.
        program::Command::Apply(path) => {
            let Some(image) = load_stable_copy(&path) else {
                return;
            };

            program::log(format!("автозапуск, картинка: {}", path.display()));

            let _ = crate::engine::ensure_com();
            start(Some(image), None, None, false);
        }

        program::Command::ShowWindow { preselect, auto_apply } => {
            let _ = crate::engine::ensure_com();
            let hwnd = main_form::create(preselect, auto_apply);
            start(None, Some(hwnd), None, true);
        }
    }
}

/// Создаёт состояние приложения и входит в цикл сообщений.
fn start(
    image: Option<DynamicImage>,
    main: Option<HWND>,
    hidden_host: Option<HWND>,
    visible: bool,
) {
    // Без окна утилиты таймеры и значок в трее принимает скрытое окно.
    let hidden_host = if main.is_none() {
        Some(main_form::create_hidden_host())
    } else {
        hidden_host
    };

    if let Some(hidden) = hidden_host {
        if hidden.0.is_null() {
            program::log("не удалось создать скрытое окно трея");
            return;
        }
        set_hidden_host(hidden);
    }

    // `create` возвращает нулевой хендл, если окно не создалось: работать не с чем.
    if main.is_some_and(|main| main.0.is_null()) {
        program::log("не удалось создать окно утилиты");
        return;
    }

    let owner = match (main, hidden_host) {
        (Some(main), _) => main,
        (_, Some(hidden)) => hidden,
        _ => {
            program::log("не удалось создать ни одного окна");
            return;
        }
    };

    APP.with(|slot| {
        *slot.borrow_mut() = Some(App {
            engine: OverlayEngine::new(owner),
            main,
            hidden_host,
            image,
            really_exit: false,
        });
    });

    set_ready(true);

    // В режиме трея оверлей поднимаем сразу: окна утилиты нет.
    if hidden_host.is_some() {
        // Картинку отдаём движку, и только после выхода из `with_app`
        // забираем ошибку: внутри заём `RefCell` уже занят.
        let error = with_app(|app| {
            if let Some(image) = app.image.take() {
                app.engine.set_image(image);
            }
            app.engine.apply(true);
            app.engine.take_error()
        });

        if let Some(error) = error {
            set_status(&error);
        }
    }

    if visible {
        unsafe {
            let _ = ShowWindow(owner, SW_SHOW);
        }
    }

    // Состояние появилось только сейчас — до этого окно создавалось вслепую.
    main_form::run_pending_auto_apply();

    message_loop();

    // Оверлей снимаем явно: сначала показываем системные обои под окном.
    with_app(|app| app.engine.remove());

    // Готовность снимаем до разрушения окон: их обработчики уже не должны
    // лезть в состояние, которого больше нет.
    set_ready(false);
    APP.with(|slot| {
        *slot.borrow_mut() = None;
    });

    if let Some(hidden) = hidden_host {
        unsafe {
            let _ = DestroyWindow(hidden);
        }
    }

    paint::release_fonts();
    crate::engine::release_com();
}

/// Обычный цикл сообщений.
fn message_loop() {
    let mut message = MSG::default();

    unsafe {
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            if message.message == WM_QUIT {
                break;
            }

            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Закрывает окно, принимающее таймеры и значок в трее.
fn close_current_window() {
    let window = with_app(|app| app.main.or(app.hidden_host));
    if let Some(window) = window {
        unsafe {
            let _ = DestroyWindow(window);
        }
    }

    unsafe {
        PostQuitMessage(0);
    }
}

/// Копия картинки для оверлея.
fn load_stable_copy(path: &std::path::Path) -> Option<DynamicImage> {
    match crate::images::stable_copy(path) {
        Ok(image) => Some(image),
        Err(error) => {
            program::log(format!("не удалось загрузить картинку: {error}"));
            dialogs::show_error(HWND::default(), &format!("Не удалось загрузить изображение:\n{error}"));
            None
        }
    }
}

