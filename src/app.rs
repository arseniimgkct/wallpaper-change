//! Склейка приложения: общее состояние, цикл сообщений и окна.

use std::cell::RefCell;

use image::DynamicImage;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostQuitMessage,
    RegisterClassExW, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, MSG, SW_HIDE,
    SW_SHOW, WM_QUIT, WNDCLASSEXW,
};
use windows::core::PCWSTR;

use crate::engine::OverlayEngine;
use crate::program::{self, utf16_with_nul};
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

/// Есть ли состояние приложения.
///
/// Пока окно создаётся, `WM_NCCREATE` и другие сообщения приходят раньше,
/// чем состояние появится: обращаться к нему в этот момент нельзя.
pub fn is_ready() -> bool {
    APP.with(|app| app.borrow().is_some())
}

/// Выполняет действие над состоянием приложения.
pub fn with_app<R>(action: impl FnOnce(&mut App) -> R) -> R {
    APP.with(|app| {
        let mut app = app.borrow_mut();
        let app = app.as_mut().expect("состояние приложения ещё не создано");
        action(app)
    })
}

/// Действие над состоянием, если оно уже есть.
pub fn try_with_app<R>(action: impl FnOnce(&mut App) -> R, fallback: R) -> R {
    APP.with(|app| {
        let mut app = app.borrow_mut();
        match app.as_mut() {
            Some(app) => action(app),
            None => fallback,
        }
    })
}

/// Картинка для оверлея.
pub fn overlay_image() -> Option<DynamicImage> {
    with_app(|app| app.image.clone())
}

/// Показывает окно утилиты из трея.
pub fn show_main_window() {
    let Some(hwnd) = with_app(|app| app.main) else {
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
    let hwnd = with_app(|app| app.main);
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

/// Является ли окно скрытым хостом для режима «только трей».
pub fn window_is_hidden_host(hwnd: HWND) -> bool {
    try_with_app(|app| app.hidden_host == Some(hwnd), false)
}

/// Запускает приложение в нужном режиме и крутит цикл сообщений.
pub fn run(command: program::Command) {
    match command {
        // Откат без окна: сделали работу и вышли.
        program::Command::Revert { restore_light } => {
            let report = reverter::revert_all(None, restore_light);
            program::log(format!("откат из командной строки: {report}"));
            return;
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

    let owner = match (main, hidden_host) {
        (Some(main), _) => Some(main),
        (_, Some(hidden)) => Some(hidden),
        _ => {
            program::log("не удалось создать ни одного окна");
            return;
        }
    };
    let owner = owner.expect("окно определено выше");

    APP.with(|slot| {
        *slot.borrow_mut() = Some(App {
            engine: OverlayEngine::new(owner),
            main,
            hidden_host,
            image,
            really_exit: false,
        });
    });

    // В режиме трея оверлей поднимаем сразу: окна утилиты нет.
    if hidden_host.is_some() {
        with_app(|app| app.engine.apply(true));
    }

    if visible {
        unsafe {
            let _ = ShowWindow(owner, SW_SHOW);
        }
    }

    message_loop();

    // Оверлей снимаем явно: сначала показываем системные обои под окном.
    with_app(|app| app.engine.remove());
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

/// Регистрирует класс окна по имени и оконной процедуре.
pub fn register_class(name: &str, wnd_proc: WNDPROC) {
    unsafe {
        let class = utf16_with_nul(name);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };

        // Повторная регистрация возвращает 0 — это не ошибка.
        RegisterClassExW(&wc);
    }
}

/// Состояние окна утилиты, если окно открыто.
pub fn main_state(hwnd: HWND) -> Option<&'static mut main_form::MainState> {
    main_form::state(hwnd)
}

/// Тип оконной процедуры Win32.
pub type WNDPROC = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

/// Обработчик по умолчанию для сообщений, которые мы не перехватываем.
pub fn default_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// Прячет окно, не закрывая его.
pub fn hide_window(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}