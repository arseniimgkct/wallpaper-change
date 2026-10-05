//! Окно утилиты: выбор картинки, пресеты цвета и все кнопки управления.
//!
//! Состояние окна живёт в `GWLP_USERDATA` и освобождается при разрушении окна,
//! поэтому `MainState` не перемещается после создания.

use std::path::{Path, PathBuf};

use image::DynamicImage;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, ReleaseCapture, SetCapture, VK_CONTROL};
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, LoadCursorW, RegisterClassExW,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW,
    CW_USEDEFAULT, GWLP_USERDATA, IDC_ARROW, IDC_WAIT, PostQuitMessage, SW_HIDE, SW_SHOW,
    WM_CLOSE, WM_DESTROY, WM_DROPFILES,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY,
    WM_PAINT, WM_TIMER, WINDOW_EX_STYLE, WINDOW_STYLE, WS_CAPTION, WS_CLIPCHILDREN,
    WS_MINIMIZEBOX, WS_OVERLAPPED, WS_POPUP, WS_SYSMENU, WNDCLASSEXW,
};
use windows::core::PCWSTR;

use crate::app;
use crate::browser;
use crate::clipboard;
use crate::engine;
use crate::images;
use crate::native;
use crate::program::{self, utf16_with_nul};
use crate::reverter;
use crate::shell;
use crate::taskbar;
use crate::taskbar_pins::{self, TaskbarResult};
use crate::theme;
use crate::ui::controls::{Action, Control, ControlState, LabelColor};
use crate::ui::paint::{self, Painter};
use crate::ui::{browser_picker, dialogs};

const CLASS_NAME: &str = "DesktopOverlayMainForm";
const HIDDEN_CLASS: &str = "DesktopOverlayHiddenHost";

/// Логический размер окна: как в оригинале, 500 x 512.
const WINDOW_WIDTH: i32 = 500;
const WINDOW_HEIGHT: i32 = 512;

/// Готовые цвета: те же, что и в оригинале.
const PRESETS: [([u8; 3], &str); 6] = [
    ([0, 0, 0], "Чёрный"),
    ([18, 18, 18], "Тёмный графит"),
    ([28, 33, 40], "Тёмно-синий"),
    ([37, 42, 52], "Сланцевый"),
    ([50, 50, 50], "Серый"),
    ([245, 245, 245], "Белый"),
];

const INITIAL_STATUS: &str = "Оверлей работает поверх политик, пока запущена программа.\n\
                              Значок приложения находится в системном трее.";

/// Состояние окна утилиты.
pub struct MainState {
    hwnd: HWND,
    controls: Vec<Control>,
    /// Выбранный файл или сгенерированная картинка цвета.
    chosen: Option<PathBuf>,
    preview: Option<DynamicImage>,
    hint_visible: bool,
    status: String,
    autostart_checked: bool,
    /// Кнопка под курсором.
    hover: Option<usize>,
    /// Нажатая кнопка.
    pressed: Option<usize>,
    /// Идёт долгая операция: повторные нажатия игнорируем.
    busy: bool,
}

impl MainState {
    fn new(hwnd: HWND) -> Box<MainState> {
        let autostart_checked = theme::autostart_is_enabled();

        let mut state = Box::new(MainState {
            hwnd,
            controls: Vec::new(),
            chosen: None,
            preview: None,
            hint_visible: true,
            status: if autostart_checked {
                "Автозапуск активен. Оверлей прикреплен к рабочему столу.".to_string()
            } else {
                INITIAL_STATUS.to_string()
            },
            autostart_checked,
            hover: None,
            pressed: None,
            busy: false,
        });

        state.rebuild_controls();
        state
    }

    /// Раскладка окна. Прямоугольники заданы в логических координатах,
    /// как в оригинале; масштаб под экран применяется при отрисовке.
    fn rebuild_controls(&mut self) {
        let r = paint::rect;

        self.controls = vec![
            Control::DropZone { rect: r(20, 16, 460, 170) },
            Control::Label {
                rect: r(20, 198, 42, 24),
                text: "Цвет:".to_string(),
                points: 8.5,
                color: LabelColor::Muted,
                bold: false,
            },
        ];

        // Квадраты-образцы идут с шагом 32 пикселя, начиная с 64.
        for (index, (color, name)) in PRESETS.iter().enumerate() {
            self.controls.push(Control::ColorSwatch {
                rect: r(64 + index as i32 * 32, 196, 26, 26),
                color: paint::rgb(color[0], color[1], color[2]),
                action: Action::PresetColor(index),
                tooltip: Some(name.to_string()),
            });
        }

        self.controls.push(Control::Button {
            rect: r(256, 196, 74, 26),
            text: "Палитра…".to_string(),
            action: Action::PickColor,
            primary: false,
            enabled: true,
            tooltip: None,
            points: 9.0,
        });

        self.controls.push(Control::Button {
            rect: r(334, 196, 74, 26),
            text: "Обзор…".to_string(),
            action: Action::PickFile,
            primary: false,
            enabled: true,
            tooltip: None,
            points: 9.0,
        });

        self.controls.push(Control::Button {
            rect: r(412, 196, 68, 26),
            text: "📋 Буфер".to_string(),
            action: Action::Paste,
            primary: false,
            enabled: true,
            tooltip: Some("Вставить картинку из буфера обмена (Ctrl+V)".to_string()),
            points: 9.0,
        });

        self.controls.push(Control::Button {
            rect: r(20, 234, 300, 34),
            text: "Установить обои".to_string(),
            action: Action::Apply,
            primary: true,
            // Пока нечего ставить, кнопка остаётся неактивной.
            enabled: self.chosen.is_some(),
            tooltip: None,
            points: 9.0,
        });

        self.controls.push(Control::Button {
            rect: r(330, 234, 150, 34),
            text: "Откатить всё".to_string(),
            action: Action::Revert,
            primary: false,
            enabled: true,
            tooltip: None,
            points: 9.0,
        });

        self.controls.push(Control::Button {
            rect: r(20, 278, 460, 34),
            text: theme_button_text(),
            action: Action::ToggleTheme,
            primary: false,
            enabled: true,
            tooltip: None,
            points: 9.0,
        });

        self.controls.push(Control::Button {
            rect: r(20, 320, 460, 34),
            text: taskbar_button_text(),
            action: Action::ToggleTaskbar,
            primary: false,
            enabled: taskbar::is_supported(),
            tooltip: taskbar_tooltip(),
            points: 9.0,
        });

        self.controls.push(Control::Button {
            rect: r(20, 362, 460, 34),
            text: edge_button_text(),
            action: Action::DetachEdge,
            primary: false,
            enabled: true,
            tooltip: Some(EDGE_TOOLTIP.to_string()),
            points: 9.0,
        });

        self.controls.push(Control::Checkbox {
            rect: r(20, 406, 460, 24),
            text: "Запускать вместе с Windows".to_string(),
            action: Action::ToggleAutoStart,
            checked: self.autostart_checked,
        });

        let status = self.status.clone();
        self.controls.push(Control::Label {
            rect: r(20, 438, 460, 65),
            text: status,
            points: 8.5,
            color: LabelColor::Dim,
            bold: false,
        });
    }

    fn status_label_index(&self) -> Option<usize> {
        self.controls.iter().position(|control| {
            matches!(control, Control::Label { color: LabelColor::Dim, .. })
        })
    }

    pub fn set_status(&mut self, message: &str) {
        self.status = message.to_string();

        if let Some(index) = self.status_label_index() {
            if let Control::Label { text, .. } = &mut self.controls[index] {
                *text = self.status.clone();
            }
        }

        self.invalidate();
    }

    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    /// Показывает окно.
    pub fn show(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
        }
    }

    /// Кнопка под координатами.
    fn hit(&self, x: i32, y: i32) -> Option<usize> {
        self.controls
            .iter()
            .position(|control| control.is_enabled() && control.contains(x, y))
    }

    fn set_hover(&mut self, index: Option<usize>) {
        if self.hover != index {
            self.hover = index;
            self.invalidate();
        }
    }

    /// Выбирает файл и обновляет превью.
    pub fn select_file(&mut self, path: &Path, is_solid_color: bool, color_hex: Option<&str>) {
        match images::probe(path) {
            Some(image) => self.preview = Some(image),
            None => {
                self.preview = None;
                self.hint_visible = true;
                self.chosen = None;
                self.rebuild_controls();
                self.set_status(&format!("Не удалось открыть файл: {}", path.display()));
                return;
            }
        }

        self.chosen = Some(path.to_path_buf());
        self.hint_visible = false;

        let message = if is_solid_color {
            let hex = color_hex.unwrap_or("");
            format!("Выбран сплошной цвет: {hex} (нажмите «Установить обои»)")
        } else {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            format!("Выбран файл: {name}")
        };

        self.rebuild_controls();
        self.set_status(&message);
    }

    /// Ставит выбранную картинку на рабочий стол.
    pub fn apply_wallpaper(&mut self, success: Option<&str>) {
        let Some(chosen) = self.chosen.clone() else {
            return;
        };

        let image = match images::stable_copy(&chosen) {
            Ok(image) => image,
            Err(error) => {
                self.set_status(&format!("Не удалось загрузить изображение: {error}"));
                dialogs::show_error(
                    self.hwnd,
                    &format!("Не удалось загрузить изображение:\n{error}"),
                );
                return;
            }
        };

        // Картинку кладём в состояние приложения: оттуда её берёт и движок,
        // которому нужно пересоздавать оверлей после перезапуска Проводника.
        let applied = app::with_app(|app| {
            app.image = Some(image);
            app.engine.apply(true)
        });

        if !applied {
            self.set_status("Не удалось встроиться в рабочий стол. Подробности в overlay.log.");
            return;
        }

        self.autostart_checked = theme::autostart_is_enabled();
        self.rebuild_controls();
        self.set_status(success.unwrap_or("Обои успешно установлены поверх системных."));
    }

    /// Вставляет картинку из буфера обмена и сразу ставит её.
    pub fn paste_from_clipboard(&mut self) {
        let path = match clipboard::try_take() {
            Ok(path) => path,
            Err(error) => {
                self.set_status(&error);
                return;
            }
        };

        self.select_file(&path, false, None);
        if self.chosen.is_none() {
            return;
        }

        self.set_status("Картинка из буфера обмена выбрана, устанавливаю...");
        self.apply_wallpaper(Some("Картинка из буфера обмена установлена на рабочий стол."));
    }

    /// Генерирует картинку сплошного цвета и выбирает её.
    pub fn set_solid_color(&mut self, color: [u8; 3]) {
        match images::solid_color(color) {
            Ok(path) => self.select_file(&path, true, Some(&images::hex(color))),
            Err(error) => self.set_status(&format!("Ошибка генерации цвета: {error}")),
        }
    }

    /// Полный откат с подтверждениями.
    pub fn revert(&mut self) {
        if !dialogs::confirm(
            self.hwnd,
            "Убрать оверлей с рабочего стола и вернуть системные обои?",
            "Откатить обои",
        ) {
            return;
        }

        let also_light = dialogs::confirm(
            self.hwnd,
            "Также вернуть светлую тему Windows?",
            "Тема оформления",
        );

        self.perform_revert(also_light);
    }

    fn perform_revert(&mut self, restore_light: bool) {
        let report = app::with_app(|app| reverter::revert_all(Some(&mut app.engine), restore_light));

        self.preview = None;
        self.hint_visible = true;
        self.chosen = None;
        self.autostart_checked = theme::autostart_is_enabled();

        self.rebuild_controls();
        self.set_status(&format!("Откат выполнен: {report}."));
    }

    /// Переключение темы Windows.
    fn toggle_windows_theme(&mut self) {
        let dark = !theme::is_dark_theme();
        self.set_status("Переключение темы Windows и перезапуск проводника...");
        self.begin_busy();
        engine::pump_messages();

        let applied = theme::apply_and_restart_explorer(dark);

        self.busy = false;
        self.set_default_cursor();
        self.rebuild_controls();

        let message = if applied {
            format!(
                "Тема переключена: {}. Проводник перезапущен.",
                if dark { "Тёмная" } else { "Светлая" }
            )
        } else {
            "Не удалось переключить тему Windows — проверьте права записи в реестр.".to_string()
        };
        self.set_status(&message);
    }

    /// Уменьшение и восстановление панели задач.
    fn toggle_taskbar(&mut self) {
        if !taskbar::is_supported() {
            self.set_status(taskbar::is_supported_message());
            return;
        }

        let target = taskbar::is_small() != Some(true);

        self.set_status("Меняю размер панели задач и перезапускаю Проводник...");
        self.begin_busy();

        if !taskbar::set_small(target) {
            self.busy = false;
            self.set_default_cursor();
            self.set_status("Не удалось изменить панель задач — проверьте права записи в реестр.");
            return;
        }

        self.set_status(if target {
            "Панель задач уменьшена. Проводник перезапущен."
        } else {
            "Панель задач возвращена к обычному размеру. Проводник перезапущен."
        });

        shell::restart_explorer();
        std::thread::sleep(std::time::Duration::from_millis(400));
        engine::pump_messages();
        native::redraw_desktop();

        self.busy = false;
        self.set_default_cursor();
        self.rebuild_controls();
    }

    /// Открепляет Edge и передаёт браузер по умолчанию выбранному.
    fn detach_edge_and_pick_browser(&mut self) {
        let browsers = browser::detect_alternatives();
        if browsers.is_empty() {
            self.set_status(
                "Chrome и Firefox не найдены. Установите один из них, чтобы сменить \
                 браузер по умолчанию.",
            );
            return;
        }

        let Some(chosen) = browser_picker::show(self.hwnd, &browsers) else {
            return;
        };

        self.set_status(&format!(
            "Открепляю Microsoft Edge и передаю браузер {}...",
            chosen.title
        ));
        self.begin_busy();

        let mut taskbar_report = "Microsoft Edge и так не был закреплён на панели задач.".to_string();
        if taskbar_pins::is_pinned(taskbar_pins::EDGE_NAME) {
            let (result, report) = taskbar_pins::try_unpin(taskbar_pins::EDGE_NAME);
            taskbar_report = report;

            if result == TaskbarResult::DoneNeedsExplorerRestart {
                shell::restart_explorer();
                std::thread::sleep(std::time::Duration::from_millis(400));
                engine::pump_messages();
                native::redraw_desktop();

                taskbar_report = if taskbar_pins::is_pinned(taskbar_pins::EDGE_NAME) {
                    "Microsoft Edge откреплён от панели задач.".to_string()
                } else {
                    "Панель задач перезапущена, Microsoft Edge откреплён.".to_string()
                };
            }
        }

        self.rebuild_controls();

        let (_, browser_report) = browser::make_default(&chosen);
        self.busy = false;
        self.set_default_cursor();
        self.set_status(&format!("{browser_report} {taskbar_report}"));
    }

    /// Включение и выключение автозапуска.
    fn toggle_autostart(&mut self) {
        match theme::set_autostart(!self.autostart_checked) {
            Ok(()) => {
                self.autostart_checked = theme::autostart_is_enabled();
                self.rebuild_controls();
                self.set_status(if self.autostart_checked {
                    "Автозапуск включён."
                } else {
                    "Автозапуск выключен."
                });
            }
            Err(error) => {
                // Флажок возвращается в прежнее состояние: включить не вышло.
                self.rebuild_controls();
                self.set_status(&error);
                dialogs::show_warning(self.hwnd, &error);
            }
        }
    }

    /// Вход в долгую операцию: курсор «часы», окно не перерисовывается заново.
    fn begin_busy(&mut self) {
        self.busy = true;
        self.set_wait_cursor();
    }

    fn set_wait_cursor(&self) {
        unsafe {
            if let Ok(cursor) = LoadCursorW(None, IDC_WAIT) {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetCursor(Some(cursor));
            }
        }
    }

    fn set_default_cursor(&self) {
        unsafe {
            if let Ok(cursor) = LoadCursorW(None, IDC_ARROW) {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetCursor(Some(cursor));
            }
        }
    }

    /// Видно ли окно.
    pub fn is_visible(&self) -> bool {
        unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(self.hwnd).as_bool() }
    }

    /// Скрыть окно в трей.
    pub fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
}

/// Подсказка кнопки браузера: объясняет, что произойдёт.
const EDGE_TOOLTIP: &str = "Передаёт http, https и .htm/.html выбранному браузеру и снимает \
                           Microsoft Edge с панели задач. Firefox назначит себя сам, Chrome и Edge \
                           попросят подтвердить в своей странице настроек.";

/// Текст кнопки темы.
fn theme_button_text() -> String {
    if theme::is_dark_theme() {
        "☀ Сменить тему Windows на светлую".to_string()
    } else {
        "🌙 Сменить тему Windows на тёмную".to_string()
    }
}

/// Текст кнопки панели задач.
fn taskbar_button_text() -> String {
    if !taskbar::is_supported() {
        return "⤢ Уменьшить панель задач (недоступно)".to_string();
    }

    if taskbar::is_small() == Some(true) {
        "⤢ Вернуть обычную панель задач".to_string()
    } else {
        "⤢ Уменьшить панель задач".to_string()
    }
}

fn taskbar_tooltip() -> Option<String> {
    if !taskbar::is_supported() {
        return Some(taskbar::is_supported_message().to_string());
    }
    Some("Панель задач станет ниже на 10 пикселей. Проводник перезапустится.".to_string())
}

/// Текст кнопки браузера.
fn edge_button_text() -> String {
    let current = browser::current_title();

    if taskbar_pins::is_pinned(taskbar_pins::EDGE_NAME) {
        format!("🌐 Открепить Edge и выбрать браузер (сейчас: {current})")
    } else {
        format!("🌐 Edge откреплён, браузер по умолчанию: {current}")
    }
}

/// Создаёт окно утилиты и кладёт состояние в `GWLP_USERDATA`.
pub fn create(preselect: Option<PathBuf>, auto_apply: bool) -> HWND {
    register_class(CLASS_NAME);

    let title = utf16_with_nul(&format!("Обои рабочего стола — {}", program::APP_NAME));
    let class = utf16_with_nul(CLASS_NAME);
    let dpi = window_dpi();

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WINDOW_STYLE(
                WS_OVERLAPPED.0
                    | WS_CAPTION.0
                    | WS_SYSMENU.0
                    | WS_MINIMIZEBOX.0
                    | WS_CLIPCHILDREN.0,
            ),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            scaled(WINDOW_WIDTH, dpi as i32),
            scaled(WINDOW_HEIGHT, dpi),
            None,
            None,
            GetModuleHandleW(None).ok().map(Into::into),
            None,
        )
    };

    let hwnd = match hwnd {
        Ok(hwnd) => hwnd,
        Err(e) => {
            program::log(format!("не удалось создать окно утилиты: {e}"));
            dialogs::show_error(HWND::default(), &format!("Не удалось создать окно:\n{e}"));
            return HWND::default();
        }
    };

    unsafe {
        DragAcceptFiles(hwnd, true);
    }

    // Состояние не перемещается после этого момента: окно владеет им.
    let mut boxed: Box<MainState> = MainState::new(hwnd);
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&mut *boxed as *mut MainState) as isize);
    }
    std::mem::forget(boxed);

    center_window(hwnd);

    if let Some(path) = preselect {
        if let Some(window_state) = state(hwnd) {
            window_state.select_file(&path, false, None);
            if auto_apply {
                window_state.apply_wallpaper(None);
            }
        }
    }

    hwnd
}

/// Скрытое окно для режима «только трей»: принимает таймеры и значок.
pub fn create_hidden_host() -> HWND {
    register_class(HIDDEN_CLASS);

    let class = utf16_with_nul(HIDDEN_CLASS);
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class.as_ptr()),
            PCWSTR::null(),
            WINDOW_STYLE(WS_POPUP.0),
            0,
            0,
            0,
            0,
            None,
            None,
            GetModuleHandleW(None).ok().map(Into::into),
            None,
        )
        .unwrap_or_default()
    }
}

/// Состояние окна по `GWLP_USERDATA`.
///
/// Возвращается `'static`, потому что окно владеет состоянием до
/// `WM_NCDESTROY`: обращаться к нему можно только из оконной процедуры.
pub fn state(hwnd: HWND) -> Option<&'static mut MainState> {
    let pointer = app::window_long(hwnd);
    if pointer == 0 {
        return None;
    }
    Some(unsafe { &mut *(pointer as *mut MainState) })
}

/// Регистрация класса окна утилиты или скрытого окна.
fn register_class(name: &str) {
    let class = utf16_with_nul(name);

    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(wnd_proc),
            hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };

        // Повторная регистрация возвращает 0 — это не ошибка.
        RegisterClassExW(&wc);
    }
}

fn window_dpi() -> i32 {
    use windows::Win32::UI::HiDpi::GetDpiForSystem;
    unsafe { GetDpiForSystem().max(96) as i32 }
}

fn scaled(value: i32, dpi: i32) -> i32 {
    (value as f64 * dpi as f64 / 96.0).round() as i32
}

fn center_window(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER};

    let screen = program::primary_screen_rect();
    let Some(rect) = program::window_rect(hwnd) else {
        return;
    };

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    let x = screen.left + (screen.right - screen.left - width) / 2;
    let y = screen.top + (screen.bottom - screen.top - height) / 2;

    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER,
        );
    }
}

/// Обработчик сообщений окна утилиты и скрытого окна трея.
pub unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Пока создаётся окно, состояния приложения ещё нет: сообщения
    // `WM_NCCREATE` и `WM_CREATE` приходят раньше, чем его удалось создать.
    // На них нужно ответить, не заглядывая в состояние.
    if !app::is_ready() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }

    // У скрытого окна нет состояния: только таймеры и значок в трее.
    if app::window_is_hidden_host(hwnd) {
        return hidden_host_proc(hwnd, message, wparam, lparam);
    }

    // Значок в трее есть и у окна утилиты.
    if message == engine::TRAY_MESSAGE {
        let exit = app::with_app(|app| app.engine.on_tray_message(lparam));
        if exit {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        return LRESULT(0);
    }

    match message {
        WM_PAINT => {
            paint(hwnd);
            return LRESULT(0);
        }

        // Фон рисуем целиком в WM_PAINT, поэтому стереть его не нужно.
        WM_ERASEBKGND => return LRESULT(1),

        WM_MOUSEMOVE => {
            let (x, y) = point_of(lparam);
            if let Some(state) = state(hwnd) {
                let hit = state.hit(x, y);
                state.set_hover(hit);

                // Пока кнопка зажата, окно должно получать движение мыши
                // даже если курсор ушёл за его пределы.
                if state.pressed.is_some() {
                    let _ = SetCapture(hwnd);
                }
            }
            return LRESULT(0);
        }

        WM_LBUTTONDOWN => {
            if let Some(state) = state(hwnd) {
                if state.busy {
                    return LRESULT(0);
                }

                let (x, y) = point_of(lparam);
                state.pressed = state.hit(x, y);
                state.invalidate();

                if state.pressed.is_some() {
                    let _ = SetCapture(hwnd);
                }
            }
            return LRESULT(0);
        }

        WM_LBUTTONUP => {
            let _ = ReleaseCapture();

            let (x, y) = point_of(lparam);
            let mut action = None;

            if let Some(state) = state(hwnd) {
                let hit = state.hit(x, y);
                let pressed = state.pressed;
                state.pressed = None;

                // Срабатывает только если отпустили над той же кнопкой.
                if hit.is_some() && hit == pressed {
                    action = state.controls[hit.unwrap()].action();
                }
                state.invalidate();
            }

            if let Some(action) = action {
                run_action(hwnd, action);
            }
            return LRESULT(0);
        }

        // Ctrl+V вставляет картинку из буфера.
        WM_KEYDOWN => {
            if wparam.0 as u32 == b'V' as u32 {
                let control = GetKeyState(VK_CONTROL.0 as i32);
                if control < 0 {
                    if let Some(state) = state(hwnd) {
                        state.paste_from_clipboard();
                    }
                    return LRESULT(0);
                }
            }
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }

        WM_DROPFILES => {
            let drop = HDROP(lparam.0 as *mut core::ffi::c_void);
            let path = resolve_drop(drop);
            DragFinish(drop);

            if let Some(path) = path {
                if let Some(state) = state(hwnd) {
                    state.select_file(&path, false, None);
                }
            }
            return LRESULT(0);
        }

        WM_TIMER => {
            app::with_app(|app| app.engine.on_timer(wparam.0));
            return LRESULT(0);
        }

        // Закрытие окна прячет его в трей, а не завершает программу.
        WM_CLOSE => {
            if !app::with_app(|app| app.really_exit) {
                let _ = ShowWindow(hwnd, SW_HIDE);
                return LRESULT(0);
            }
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }

        WM_DESTROY => {
            if !app::with_app(|app| app.really_exit) {
                let _ = ShowWindow(hwnd, SW_HIDE);
                return LRESULT(0);
            }
            app::with_app(|app| app.engine.remove());
            PostQuitMessage(0);
            return LRESULT(0);
        }

        // Окно уничтожено: освобождаем состояние.
        WM_NCDESTROY => {
            if let Some(state) = state(hwnd) {
                drop(Box::from_raw(state as *mut MainState));
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }

        _ => {}
    }

    DefWindowProcW(hwnd, message, wparam, lparam)
}

/// Обработчик скрытого окна: таймеры и значок в трее.
unsafe fn hidden_host_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TIMER => {
            app::with_app(|app| app.engine.on_timer(wparam.0));
            LRESULT(0)
        }

        engine::TRAY_MESSAGE => {
            let exit = app::with_app(|app| app.engine.on_tray_message(lparam));
            if exit {
                let _ = DestroyWindow(hwnd);
                PostQuitMessage(0);
            }
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// Координаты мыши из `lParam`.
fn point_of(lparam: LPARAM) -> (i32, i32) {
    ((lparam.0 as u16) as i32, ((lparam.0 >> 16) as u16) as i32)
}

/// Выполняет действие кнопки.
fn run_action(hwnd: HWND, action: Action) {
    match action {
        Action::PickFile => {
            if let Some(path) = dialogs::open_image_file(hwnd) {
                if let Some(state) = state(hwnd) {
                    state.select_file(&path, false, None);
                }
            }
        }

        Action::Paste => {
            if let Some(state) = state(hwnd) {
                state.paste_from_clipboard();
            }
        }

        Action::PickColor => {
            if let Some(color) = dialogs::choose_color(hwnd, [18, 18, 18]) {
                if let Some(state) = state(hwnd) {
                    state.set_solid_color(color);
                }
            }
        }

        Action::PresetColor(index) => {
            if let Some((color, _)) = PRESETS.get(index).copied() {
                if let Some(state) = state(hwnd) {
                    state.set_solid_color(color);
                }
            }
        }

        Action::Apply => {
            if let Some(state) = state(hwnd) {
                state.apply_wallpaper(None);
            }
        }

        Action::Revert => {
            if let Some(state) = state(hwnd) {
                state.revert();
            }
        }

        Action::ToggleTheme => {
            if let Some(state) = state(hwnd) {
                state.toggle_windows_theme();
            }
        }

        Action::ToggleTaskbar => {
            if let Some(state) = state(hwnd) {
                state.toggle_taskbar();
            }
        }

        Action::DetachEdge => {
            if let Some(state) = state(hwnd) {
                state.detach_edge_and_pick_browser();
            }
        }

        Action::ToggleAutoStart => {
            if let Some(state) = state(hwnd) {
                state.toggle_autostart();
            }
        }

        Action::ClosePicker => {}
    }
}

/// Отрисовка окна.
unsafe fn paint(hwnd: HWND) {
    let Some(state) = state(hwnd) else {
        return;
    };

    let mut paint_struct = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut paint_struct);
    if hdc.is_invalid() {
        return;
    }

    let painter = Painter::new(hdc, window_dpi());
    painter.fill(
        paint::rect(0, 0, WINDOW_WIDTH, WINDOW_HEIGHT),
        paint::BACKGROUND,
    );

    // Превью копируем, чтобы не держать два заимствования состояния.
    let preview = state.preview.clone();
    let control_state = ControlState {
        preview: preview.as_ref(),
        hint_visible: state.hint_visible,
        background: paint::BACKGROUND,
        drop_border: paint::rgb(42, 42, 42),
    };

    for (index, control) in state.controls.iter().enumerate() {
        control.paint(
            &painter,
            state.hover == Some(index),
            state.pressed == Some(index),
            &control_state,
        );
    }

    let _ = EndPaint(hwnd, &paint_struct);
}

/// Картинка из перетаскивания: первый подходящий файл или первая в папке.
fn resolve_drop(drop: HDROP) -> Option<PathBuf> {
    // 0xFFFFFFFF — запрос «сколько всего файлов», без чтения имени.
    let count = unsafe { DragQueryFileW(drop, 0xFFFF_FFFF, None) };

    for index in 0..count {
        let length = unsafe { DragQueryFileW(drop, index, None) } as usize;
        if length == 0 {
            continue;
        }

        let mut buffer = vec![0u16; length + 1];
        unsafe {
            DragQueryFileW(drop, index, Some(&mut buffer));
        }

        let path = PathBuf::from(program::from_pwstr(Some(windows::core::PWSTR(
            buffer.as_mut_ptr(),
        ))));

        if path.is_file() && program::is_supported_image(&path) {
            return Some(path);
        }

        if path.is_dir() {
            let mut found: Vec<PathBuf> = std::fs::read_dir(&path)
                .ok()?
                .flatten()
                .map(|entry| entry.path())
                .filter(|candidate| program::is_supported_image(candidate))
                .collect();

            found.sort_by_key(|candidate| candidate.to_string_lossy().to_lowercase());
            if let Some(first) = found.into_iter().next() {
                return Some(first);
            }
        }
    }

    None
}

