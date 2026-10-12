//! Окно утилиты: выбор картинки, пресеты цвета и все кнопки управления.
//!
//! Состояние окна живёт в `GWLP_USERDATA` и освобождается при разрушении окна,
//! поэтому `MainState` не перемещается после создания.

use std::path::{Path, PathBuf};

use image::DynamicImage;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, ReleaseCapture, SetCapture, VK_CONTROL};
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, LoadCursorW, RegisterClassExW,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW,
    CW_USEDEFAULT, GWLP_USERDATA, IDC_ARROW, IDC_WAIT, PostQuitMessage, SW_HIDE, SW_SHOW,
    WM_NCCREATE,
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
use crate::ui::controls::{self, Action, Control, ControlState, LabelColor};
use crate::ui::paint::{self, Painter};
use crate::ui::{browser_picker, dialogs, tooltip};

const CLASS_NAME: &str = "DesktopOverlayMainForm";
const HIDDEN_CLASS: &str = "DesktopOverlayHiddenHost";

const TIMER_TOOLTIP: usize = 3;

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
    /// Превью в BGRA, пересчитанное один раз на текущий размер окна.
    preview: Option<controls::Preview>,
    /// Исходная картинка: из неё превью строится заново при смене размера окна.
    source: Option<DynamicImage>,
    /// Кегль, по которому построены шрифты и превью окна.
    dpi: i32,
    hint_visible: bool,
    status: String,
    autostart_checked: bool,
    /// Кнопка под курсором.
    hover: Option<usize>,
    /// Нажатая кнопка.
    pressed: Option<usize>,
    /// Идёт долгая операция: повторные нажатия игнорируем.
    busy: bool,
    /// Окно подсказки.
    tooltip: tooltip::Tooltip,
    /// Элемент, под которым сейчас показывается или будет показана подсказка.
    tooltip_target: Option<usize>,
}

impl MainState {
    fn new(hwnd: HWND) -> Box<MainState> {
        let autostart_checked = theme::autostart_is_enabled();

        let mut state = Box::new(MainState {
            hwnd,
            controls: Vec::new(),
            chosen: None,
            preview: None,
            source: None,
            dpi: window_dpi(),
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
            tooltip: tooltip::Tooltip::create(hwnd),
            tooltip_target: None,
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

    /// Прямоугольник зоны перетаскивания в логических координатах окна.
    fn drop_zone_rect(&self) -> RECT {
        self.controls
            .iter()
            .find(|control| matches!(control, Control::DropZone { .. }))
            .map(Control::rect)
            .unwrap_or_default()
    }

    /// Пересчитывает превью под текущий размер окна.
    ///
    /// Вызывается из отрисовки: пересчёт дорогой, поэтому он выполняется
    /// только когда сменилась картинка или размер окна. Иначе каждое
    /// движение мыши пересобирало бы картинку заново.
    fn refresh_preview(&mut self, dpi: i32) {
        let rect = paint::scale_rect(self.drop_zone_rect(), dpi);

        let up_to_date = match &self.preview {
            Some(preview) => preview.width == rect.right - rect.left,
            None => false,
        };

        if up_to_date {
            return;
        }

        self.preview = self
            .source
            .as_ref()
            .and_then(|source| controls::Preview::build(source, rect));
    }

    /// Запоминает картинку и сразу строит по ней превью.
    fn set_preview(&mut self, image: DynamicImage) {
        let rect = paint::scale_rect(self.drop_zone_rect(), self.dpi);
        self.preview = controls::Preview::build(&image, rect);
        self.source = Some(image);
    }

    /// Показывает окно.
    pub fn show(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
        }
    }

    /// Кнопка под координатами.
    ///
    /// Координаты мыши приходят в пикселях окна, а раскладка задана в
    /// логических координатах, поэтому прямоугольники масштабируются здесь же.
    fn hit(&self, x: i32, y: i32) -> Option<usize> {
        self.controls.iter().position(|control| {
            control.is_enabled() && paint::contains(control.scaled_rect(self.dpi), x, y)
        })
    }

    fn set_hover(&mut self, index: Option<usize>) {
        if self.hover == index {
            return;
        }

        self.hover = index;

        // Подсказка показывается для того элемента, на котором мышь остановилась.
        let target = index.filter(|i| {
            self.controls[*i].tooltip().is_some() && self.controls[*i].is_enabled()
        });

        self.schedule_tooltip(target);
        self.invalidate();
    }

    fn schedule_tooltip(&mut self, target: Option<usize>) {
        if self.tooltip_target != target {
            self.tooltip.hide();
        }

        self.tooltip_target = target;

        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(
                Some(self.hwnd),
                TIMER_TOOLTIP,
            );
        }

        if target.is_some() {
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::SetTimer(
                    Some(self.hwnd),
                    TIMER_TOOLTIP,
                    tooltip::SHOW_DELAY_MS,
                    None,
                );
            }
        }
    }

    fn on_tooltip_timer(&mut self) {
        let Some(index) = self.tooltip_target else {
            return;
        };

        // Элемент мог исчезнуть при перестроении раскладки.
        let Some(control) = self.controls.get(index) else {
            self.schedule_tooltip(None);
            return;
        };

        let Some(text) = control.tooltip().map(str::to_string) else {
            self.schedule_tooltip(None);
            return;
        };

        self.tooltip.show_below(&text, control.rect());
        self.tooltip_target = None;
    }

    /// Выбирает файл и обновляет превью.
    pub fn select_file(&mut self, path: &Path, is_solid_color: bool, color_hex: Option<&str>) {
        match images::probe(path) {
            Some(image) => self.set_preview(image),
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
    ///
    /// Заём `MainState` берётся порциями: внутри этой операции крутятся
    /// собственные циклы сообщений диалогов, и держать `&mut` на всё время
    /// означало бы второй заём того же состояния из `WM_PAINT`.
    pub fn apply_wallpaper(hwnd: HWND, success: Option<&str>) {
        let Some(chosen) = with_state(hwnd, |state| state.chosen.clone()) else {
            return;
        };
        let chosen = match chosen {
            Some(chosen) => chosen,
            None => return,
        };

        let image = match images::stable_copy(&chosen) {
            Ok(image) => image,
            Err(error) => {
                with_state(hwnd, |state| {
                    state.set_status(&format!("Не удалось загрузить изображение: {error}"));
                });
                dialogs::show_error(
                    hwnd,
                    &format!("Не удалось загрузить изображение:\n{error}"),
                );
                return;
            }
        };

        // Картинку отдаём движку: он пересоздаёт оверлей сам, когда Проводник
        // перезапустит свои окна. Ошибку забираем после выхода из `with_app`,
        // иначе движок полез бы в занятый `RefCell`.
        let (applied, error) = app::with_app(|app| {
            app.image = Some(image);
            let applied = app.engine.apply(true);
            (applied, app.engine.take_error())
        });

        with_state(hwnd, |state| {
            if !applied {
                state.set_status(error.as_deref().unwrap_or(
                    "Не удалось встроиться в рабочий стол. Подробности в overlay.log.",
                ));
                return;
            }

            state.autostart_checked = theme::autostart_is_enabled();
            state.rebuild_controls();
            state
                .set_status(success.unwrap_or("Обои успешно установлены поверх системных."));
        });
    }

    /// Вставляет картинку из буфера обмена и сразу ставит её.
    pub fn paste_from_clipboard(hwnd: HWND) {
        let path = match clipboard::try_take() {
            Ok(path) => path,
            Err(error) => {
                with_state(hwnd, |state| state.set_status(&error));
                return;
            }
        };

        let chosen = with_state(hwnd, |state| {
            state.select_file(&path, false, None);
            state.chosen.is_some()
        });
        if chosen != Some(true) {
            return;
        }

        with_state(hwnd, |state| {
            state.set_status("Картинка из буфера обмена выбрана, устанавливаю...")
        });
        MainState::apply_wallpaper(
            hwnd,
            Some("Картинка из буфера обмена установлена на рабочий стол."),
        );
    }

    /// Генерирует картинку сплошного цвета и выбирает её.
    pub fn set_solid_color(hwnd: HWND, color: [u8; 3]) {
        with_state(hwnd, |state| match images::solid_color(color) {
            Ok(path) => state.select_file(&path, true, Some(&images::hex(color))),
            Err(error) => state.set_status(&format!("Ошибка генерации цвета: {error}")),
        });
    }

    /// Полный откат с подтверждениями.
    pub fn revert(hwnd: HWND) {
        if !dialogs::confirm(
            hwnd,
            "Убрать оверлей с рабочего стола и вернуть системные обои?",
            "Откатить обои",
        ) {
            return;
        }

        let also_light = dialogs::confirm(
            hwnd,
            "Также вернуть светлую тему Windows?",
            "Тема оформления",
        );

        // Оверлей снимаем коротким заёмом, а сам откат — уже без него: внутри
        // перезапускается Проводник и крутятся сообщения.
        app::with_app(|app| app.engine.remove());
        let report = reverter::revert_all(None, also_light);

        with_state(hwnd, |state| {
            state.preview = None;
            state.hint_visible = true;
            state.chosen = None;
            state.autostart_checked = theme::autostart_is_enabled();

            state.rebuild_controls();
            state.set_status(&format!("Откат выполнен: {report}."));
        });
    }

    /// Переключение темы Windows.
    pub fn toggle_windows_theme(hwnd: HWND) {
        let dark = !theme::is_dark_theme();
        with_state(hwnd, |state| {
            state.set_status("Переключение темы Windows и перезапуск проводника...")
        });
        begin_busy(hwnd);
        engine::pump_messages();

        let applied = theme::apply_and_restart_explorer(dark);

        end_busy(hwnd);

        with_state(hwnd, |state| {
            state.rebuild_controls();
            state.set_status(&if applied {
                format!(
                    "Тема переключена: {}. Проводник перезапущен.",
                    if dark { "Тёмная" } else { "Светлая" }
                )
            } else {
                "Не удалось переключить тему Windows — проверьте права записи в реестр."
                    .to_string()
            });
        });
    }

    /// Уменьшение и восстановление панели задач.
    pub fn toggle_taskbar(hwnd: HWND) {
        if !taskbar::is_supported() {
            with_state(hwnd, |state| state.set_status(taskbar::is_supported_message()));
            return;
        }

        let target = taskbar::is_small() != Some(true);

        with_state(hwnd, |state| {
            state.set_status("Меняю размер панели задач и перезапускаю Проводник...")
        });
        begin_busy(hwnd);

        if !taskbar::set_small(target) {
            end_busy(hwnd);
            with_state(hwnd, |state| {
                state.set_status(
                    "Не удалось изменить панель задач — проверьте права записи в реестр.",
                )
            });
            return;
        }

        with_state(hwnd, |state| {
            state.set_status(if target {
                "Панель задач уменьшена. Проводник перезапущен."
            } else {
                "Панель задач возвращена к обычному размеру. Проводник перезапущен."
            })
        });

        shell::restart_explorer();
        std::thread::sleep(std::time::Duration::from_millis(400));
        engine::pump_messages();
        native::redraw_desktop();

        end_busy(hwnd);
        with_state(hwnd, |state| state.rebuild_controls());
    }

    /// Открепляет Edge и передаёт браузер по умолчанию выбранному.
    pub fn detach_edge_and_pick_browser(hwnd: HWND) {
        let browsers = browser::detect_alternatives();
        if browsers.is_empty() {
            with_state(hwnd, |state| {
                state.set_status(
                    "Chrome и Firefox не найдены. Установите один из них, чтобы сменить \
                     браузер по умолчанию.",
                )
            });
            return;
        }

        // Окно выбора крутит собственный цикл сообщений: заём состояния на
        // это время брать нельзя.
        let Some(chosen) = browser_picker::show(hwnd, &browsers) else {
            return;
        };

        with_state(hwnd, |state| {
            state.set_status(&format!(
                "Открепляю Microsoft Edge и передаю браузер {}...",
                chosen.title
            ))
        });
        begin_busy(hwnd);

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

        with_state(hwnd, |state| state.rebuild_controls());

        let (_, browser_report) = browser::make_default(&chosen);
        end_busy(hwnd);
        with_state(hwnd, |state| {
            state.set_status(&format!("{browser_report} {taskbar_report}"))
        });
    }

    /// Включение и выключение автозапуска.
    pub fn toggle_autostart(hwnd: HWND) {
        let Some(current) = with_state(hwnd, |state| state.autostart_checked) else {
            return;
        };

        match theme::set_autostart(!current) {
            Ok(()) => {
                with_state(hwnd, |state| {
                    state.autostart_checked = theme::autostart_is_enabled();
                    state.rebuild_controls();
                    state.set_status(if state.autostart_checked {
                        "Автозапуск включён."
                    } else {
                        "Автозапуск выключен."
                    });
                });
            }
            Err(error) => {
                // Флажок возвращается в прежнее состояние: включить не вышло.
                with_state(hwnd, |state| state.rebuild_controls());
                with_state(hwnd, |state| state.set_status(&error));
                dialogs::show_warning(hwnd, &error);
            }
        }
    }
}

/// Короткий заём состояния окна.
///
/// Всё, что внутри этого заёма, обязано быть синхронным: любой вызов,
/// крутящий сообщения (`MessageBox`, `pump_messages`, модальный выбор),
/// приводит к повторному входу в `WM_PAINT` и ко второму `&mut` на то же
/// состояние.
fn with_state<R>(hwnd: HWND, action: impl FnOnce(&mut MainState) -> R) -> Option<R> {
    state(hwnd).map(action)
}

/// Вход в долгую операцию: курсор «часы», повторные нажатия игнорируем.
fn begin_busy(hwnd: HWND) {
    with_state(hwnd, |state| {
        state.busy = true;
        set_wait_cursor();
    });
}

/// Выход из долгой операции: обычный курсор и перерисовка кнопок.
fn end_busy(hwnd: HWND) {
    with_state(hwnd, |state| {
        state.busy = false;
        set_default_cursor();
    });
}

fn set_wait_cursor() {
    unsafe {
        if let Ok(cursor) = LoadCursorW(None, IDC_WAIT) {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetCursor(Some(cursor));
        }
    }
}

fn set_default_cursor() {
    unsafe {
        if let Ok(cursor) = LoadCursorW(None, IDC_ARROW) {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetCursor(Some(cursor));
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

    let style = WINDOW_STYLE(
        WS_OVERLAPPED.0
            | WS_CAPTION.0
            | WS_SYSMENU.0
            | WS_MINIMIZEBOX.0
            | WS_CLIPCHILDREN.0,
    );

    // Раскладка задана в логических координатах и рассчитана на клиентскую
    // область, поэтому размер окна нужно увеличить на рамку и заголовок.
    let (window_width, window_height) = window_size_for_client(WINDOW_WIDTH, WINDOW_HEIGHT, style, dpi);

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            window_width,
            window_height,
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
        }
    }

    // Применение откладываем: на этом этапе состояние приложения ещё не
    // создано, а `apply_wallpaper` к нему обращается.
    PENDING_AUTO_APPLY.store(auto_apply, std::sync::atomic::Ordering::Relaxed);

    hwnd
}

static PENDING_AUTO_APPLY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Применяет картинку, если её запустили с `--set`.
pub fn run_pending_auto_apply() {
    if !PENDING_AUTO_APPLY.swap(false, std::sync::atomic::Ordering::Relaxed) {
        return;
    }

    let Some(hwnd) = app::with_app(|app| app.main) else {
        return;
    };
    MainState::apply_wallpaper(hwnd, None);
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

/// Размер окна, при котором клиентская область равна запрошенной.
///
/// Раскладка нарисована в логических координатах от левого верхнего угла
/// клиентской области, поэтому задавать размер окна напрямую нельзя: рамка
/// и заголовок съедали бы часть элементов, и нижняя строка статуса
/// обрезалась бы.
fn window_size_for_client(
    client_width: i32,
    client_height: i32,
    style: WINDOW_STYLE,
    dpi: i32,
) -> (i32, i32) {
    use windows::Win32::UI::WindowsAndMessaging::AdjustWindowRectEx;

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: scaled(client_width, dpi),
        bottom: scaled(client_height, dpi),
    };

    unsafe {
        let _ = AdjustWindowRectEx(&mut rect, style, false, WINDOW_EX_STYLE(0));
    }

    (rect.right - rect.left, rect.bottom - rect.top)
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
    // Тёмный заголовок окна. Заглавная буква DWM работает и до создания
    // состояния, поэтому обрабатываем здесь, до проверки готовности.
    if message == WM_NCCREATE {
        native::enable_dark_mode_for_window(hwnd);
    }

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
        // Команду выполняем уже после выхода из `with_app`: внутри него
        // `RefCell` уже занят, повторный заём паниковал бы. `try_with_app`
        // дополнительно защищает от прихода сообщения в момент, когда
        // состояние одолжил кто-то ещё: `panic = "abort"` не оставляет
        // шанса почистить оверлей, и картинка осталась бы в обоях навсегда.
        let command = app::try_with_app(|app| app.engine.on_tray_message(lparam), engine::TrayCommand::None);

        match command {
            engine::TrayCommand::ShowWindow => app::show_main_window(),
            engine::TrayCommand::Revert => {
                app::revert_from_tray();
            }
            engine::TrayCommand::Exit => app::exit_application(),
            engine::TrayCommand::None => {}
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
                state.schedule_tooltip(None);

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
                    action = hit.and_then(|index| state.controls[index].action());
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
                    MainState::paste_from_clipboard(hwnd);
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
            if wparam.0 == TIMER_TOOLTIP {
                if let Some(state) = state(hwnd) {
                    state.on_tooltip_timer();
                }
                return LRESULT(0);
            }

            app::try_with_app(|app| app.engine.on_timer(wparam.0), ());
            return LRESULT(0);
        }

        // Закрытие окна прячет его в трей, а не завершает программу.
        WM_CLOSE => {
            if !app::try_with_app(|app| app.really_exit, true) {
                let _ = ShowWindow(hwnd, SW_HIDE);
                return LRESULT(0);
            }
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }

        WM_DESTROY => {
            if !app::try_with_app(|app| app.really_exit, true) {
                let _ = ShowWindow(hwnd, SW_HIDE);
                return LRESULT(0);
            }
            app::try_with_app(|app| app.engine.remove(), ());
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
            app::try_with_app(|app| app.engine.on_timer(wparam.0), ());
            LRESULT(0)
        }

        engine::TRAY_MESSAGE => {
            let command =
                app::try_with_app(|app| app.engine.on_tray_message(lparam), engine::TrayCommand::None);

            match command {
                engine::TrayCommand::ShowWindow => app::show_main_window(),
                engine::TrayCommand::Revert => {
                    app::revert_from_tray();
                }
                engine::TrayCommand::Exit => app::exit_application(),
                engine::TrayCommand::None => {}
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
///
/// Все действия принимают `hwnd`, а не `&mut MainState`: почти каждое
/// открывает диалог или перезапускает Проводник, а это крутит сообщения.
fn run_action(hwnd: HWND, action: Action) {
    match action {
        Action::PickFile => {
            if let Some(path) = dialogs::open_image_file(hwnd) {
                with_state(hwnd, |state| state.select_file(&path, false, None));
            }
        }

        Action::Paste => MainState::paste_from_clipboard(hwnd),

        Action::PickColor => {
            if let Some(color) = dialogs::choose_color(hwnd, [18, 18, 18]) {
                MainState::set_solid_color(hwnd, color);
            }
        }

        Action::PresetColor(index) => {
            if let Some((color, _)) = PRESETS.get(index).copied() {
                MainState::set_solid_color(hwnd, color);
            }
        }

        Action::Apply => MainState::apply_wallpaper(hwnd, None),

        Action::Revert => MainState::revert(hwnd),

        Action::ToggleTheme => MainState::toggle_windows_theme(hwnd),

        Action::ToggleTaskbar => MainState::toggle_taskbar(hwnd),

        Action::DetachEdge => MainState::detach_edge_and_pick_browser(hwnd),

        Action::ToggleAutoStart => MainState::toggle_autostart(hwnd),
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

    let dpi = state.dpi;
    let painter = Painter::new(hdc, dpi);
    painter.fill(
        paint::scale_rect(paint::rect(0, 0, WINDOW_WIDTH, WINDOW_HEIGHT), dpi),
        paint::BACKGROUND,
    );

    state.refresh_preview(dpi);

    let control_state = ControlState {
        preview: state.preview.as_ref(),
        hint_visible: state.hint_visible,
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

