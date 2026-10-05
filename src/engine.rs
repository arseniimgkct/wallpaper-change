//! Движок оверлея: следит за тем, чтобы картинка осталась на рабочем столе,
//! и держит значок в системном трее.
//!
//! Проводник периодически пересоздаёт свои окна, поэтому оверлей приходится
//! возвращать на место: этим занимается сторожевой таймер.

use image::DynamicImage;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, LoadIconW, SetForegroundWindow, SetTimer,
    TrackPopupMenu, IDI_APPLICATION, KillTimer, MF_SEPARATOR, MF_STRING, MSG, TPM_RETURNCMD,
    TPM_RIGHTBUTTON,
};
use windows::core::PCWSTR;

use crate::app;
use crate::native;
use crate::overlay::OverlayWindow;
use crate::program::{self, utf16_with_nul};

/// Сообщение, которым значок в трее сообщает о нажатии.
pub const TRAY_MESSAGE: u32 = 0x8000 + 51;

/// Как часто проверяем, что оверлей на месте.
const WATCHDOG_INTERVAL_MS: u32 = 2000;
/// Как часто принудительно перерисовываем картинку.
const REPAINT_INTERVAL_MS: u32 = 1000;

const TIMER_WATCHDOG: usize = 1;
const TIMER_REPAINT: usize = 2;

/// Идентификаторы пунктов меню трея.
pub const MENU_SHOW: usize = 1001;
pub const MENU_REATTACH: usize = 1002;
pub const MENU_REVERT: usize = 1003;
pub const MENU_EXIT: usize = 1004;

/// Движок оверлея.
pub struct OverlayEngine {
    /// Окно, получающее таймеры и сообщения от значка в трее.
    owner: HWND,
    window: Option<OverlayWindow>,
    tray: Option<TrayIcon>,
    watchdog: Option<usize>,
    repaint: Option<usize>,
    applied: bool,
    /// Последний известный размер основного монитора: по нему ловим смену
    /// разрешения, не подписываясь на системные события.
    last_screen: RECT,
}

impl OverlayEngine {
    pub fn new(owner: HWND) -> OverlayEngine {
        OverlayEngine {
            owner,
            window: None,
            tray: None,
            watchdog: None,
            repaint: None,
            applied: false,
            last_screen: program::primary_screen_rect(),
        }
    }

    /// Ставит картинку на рабочий стол. `true`, если оверлей действительно встал.
    ///
    /// Картинку кладёт вызывающий: движок берёт её из состояния приложения, чтобы
    /// сторожевой таймер мог пересоздать окно после перезапуска Проводника.
    pub fn apply(&mut self, show_tray: bool) -> bool {
        self.remove();

        self.applied = true;

        if show_tray {
            self.tray = TrayIcon::create(self.owner, TRAY_MESSAGE);
        }

        unsafe {
            self.watchdog = Some(SetTimer(
                Some(self.owner),
                TIMER_WATCHDOG,
                WATCHDOG_INTERVAL_MS,
                None,
            ));
            self.repaint = Some(SetTimer(
                Some(self.owner),
                TIMER_REPAINT,
                REPAINT_INTERVAL_MS,
                None,
            ));
        }

        self.ensure_overlay(false);
        self.is_healthy()
    }

    /// Снимает оверлей и возвращает системные обои.
    pub fn remove(&mut self) {
        self.applied = false;

        self.stop_timers();

        // Сначала показываем системные обои, иначе под окном останется
        // пустое место до перерисовки рабочего стола.
        let mut shown = false;
        if let Some(window) = self.window.as_mut() {
            if window.is_alive() && window.show_system_wallpaper() {
                shown = true;
            }
        }

        if shown {
            pump_messages();
        }

        self.window = None;
        self.destroy_tray();

        native::redraw_desktop();
    }

    /// Оверлей жив и встроен в рабочий стол.
    pub fn is_healthy(&self) -> bool {
        self.window.as_ref().is_some_and(OverlayWindow::is_still_attached)
    }

    /// Оверлей установлен, но окно потерялось.
    pub fn is_applied(&self) -> bool {
        self.applied
    }

    /// Тик таймера: сторож и перерисовка.
    pub fn on_timer(&mut self, timer_id: usize) {
        match timer_id {
            TIMER_WATCHDOG => self.ensure_overlay(false),
            TIMER_REPAINT => {
                if let Some(window) = &self.window {
                    window.force_repaint();
                }
            }
            _ => {}
        }
    }

    /// Обрабатывает нажатие значка в трее. `true`, если приложение завершено.
    pub fn on_tray_message(&mut self, lparam: LPARAM) -> bool {
        // Меню показываем по правому клику или по нажатию средней кнопки.
        let event = lparam.0 as u32;
        const WM_RBUTTONUP: u32 = 0x0205;
        const WM_CONTEXTMENU: u32 = 0x007B;

        if event != WM_RBUTTONUP && event != WM_CONTEXTMENU {
            return false;
        }

        match self.show_tray_menu() {
            Some(MENU_SHOW) => {
                app::show_main_window();
            }
            Some(MENU_REATTACH) => self.ensure_overlay(true),
            Some(MENU_REVERT) => return app::revert_from_tray(),
            Some(MENU_EXIT) => app::exit_application(),
            _ => {}
        }

        false
    }

    /// Показывает контекстное меню значка и возвращает выбранный пункт.
    fn show_tray_menu(&self) -> Option<usize> {
        let menu = unsafe { CreatePopupMenu().ok()? };

        let show = utf16_with_nul("Показать окно утилиты");
        let reattach = utf16_with_nul("Переприкрепить обои");
        let revert = utf16_with_nul("Откатить обои");
        let exit = utf16_with_nul("Выход");

        unsafe {
            let _ = AppendMenuW(menu, MF_STRING, MENU_SHOW, PCWSTR(show.as_ptr()));
            let _ = AppendMenuW(menu, MF_STRING, MENU_REATTACH, PCWSTR(reattach.as_ptr()));
            let _ = AppendMenuW(menu, MF_STRING, MENU_REVERT, PCWSTR(revert.as_ptr()));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, MENU_EXIT, PCWSTR(exit.as_ptr()));
        }

        let mut point = POINT { x: 0, y: 0 };
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point);

            // Меню должно закрываться при потере фокуса, поэтому окно
            // обязано стать на передний план.
            let _ = SetForegroundWindow(self.owner);

            let command = TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON | TPM_RETURNCMD,
                point.x,
                point.y,
                None,
                self.owner,
                None,
            );

            let _ = DestroyMenu(menu);
            (command.0 != 0).then_some(command.0 as usize)
        }
    }

    /// Убеждается, что оверлей на месте, и пересоздаёт его при необходимости.
    pub fn ensure_overlay(&mut self, force: bool) {
        if !self.applied {
            return;
        }

        // Смена разрешения требует перерасчёта, но не пересоздания окна.
        let screen = program::primary_screen_rect();
        let screen_changed = screen != self.last_screen;
        self.last_screen = screen;

        if !force && self.window.as_ref().is_some_and(OverlayWindow::is_still_attached) {
            if screen_changed {
                if let Some(window) = &self.window {
                    window.layout_to_primary_monitor();
                    window.force_repaint();
                }
            }
            return;
        }

        let reason = if force {
            "принудительно"
        } else if self.window.is_none() {
            "первичный запуск"
        } else {
            "окно потеряно (перезапуск Проводника?)"
        };

        self.recreate(reason);
    }

    fn recreate(&mut self, reason: &str) {
        program::log(&format!("пересоздание оверлея: {reason}"));

        // Старое окно закрываем до создания нового: иначе на экране окажется
        // две копии картинки.
        self.window = None;

        let Some(image) = load_overlay_image() else {
            return;
        };

        match OverlayWindow::new(image) {
            Some(window) => self.window = Some(window),
            None => {
                program::log("не удалось прикрепиться");
                app::set_status("Не удалось встроить окно в рабочий стол");
            }
        }
    }

    fn stop_timers(&mut self) {
        if let Some(timer) = self.watchdog.take() {
            unsafe {
                let _ = KillTimer(Some(self.owner), timer);
            }
        }
        if let Some(timer) = self.repaint.take() {
            unsafe {
                let _ = KillTimer(Some(self.owner), timer);
            }
        }
    }

    fn destroy_tray(&mut self) {
        if let Some(tray) = self.tray.take() {
            tray.destroy();
        }
    }
}

impl Drop for OverlayEngine {
    fn drop(&mut self) {
        self.stop_timers();
        self.destroy_tray();
    }
}

/// Картинка для оверлея берётся из состояния приложения: её кладёт туда
/// вызов «Установить обои» или автозапуск.
fn load_overlay_image() -> Option<DynamicImage> {
    app::overlay_image()
}

/// Значок в системном трее.
struct TrayIcon {
    data: NOTIFYICONDATAW,
}

impl TrayIcon {
    fn create(owner: HWND, callback: u32) -> Option<TrayIcon> {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: owner,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: callback,
            ..Default::default()
        };

        data.hIcon = app_icon().unwrap_or_default();
        copy_wide_fixed(&mut data.szTip, program::APP_NAME);
        copy_wide_fixed(&mut data.szInfo, program::APP_NAME);

        let added = unsafe { Shell_NotifyIconW(NIM_ADD, &data).as_bool() };
        if !added {
            program::log("не удалось добавить значок в трей");
            return None;
        }

        Some(TrayIcon { data })
    }

    fn destroy(&self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data);
        }
    }
}

/// Копирует строку в буфер фиксированного размера с завершающим нулём.
fn copy_wide_fixed(buffer: &mut [u16], value: &str) {
    let wide = program::wide(value);
    let length = wide.len().min(buffer.len().saturating_sub(1));
    buffer[..length].copy_from_slice(&wide[..length]);
    buffer[length] = 0;
}

/// Значок приложения; если он не нашёлся — стандартный системный.
fn app_icon() -> Option<windows::Win32::UI::WindowsAndMessaging::HICON> {
    use windows::Win32::UI::WindowsAndMessaging::{
        LoadImageW, IMAGE_ICON, LR_DEFAULTSIZE, LR_LOADFROMFILE,
    };

    if let Some(exe) = program::exe_path() {
        let path = utf16_with_nul(&exe.to_string_lossy());
        unsafe {
            if let Ok(handle) = LoadImageW(
                None,
                PCWSTR(path.as_ptr()),
                IMAGE_ICON,
                0,
                0,
                LR_LOADFROMFILE | LR_DEFAULTSIZE,
            ) {
                return Some(windows::Win32::UI::WindowsAndMessaging::HICON(handle.0));
            }
        }
    }

    unsafe { LoadIconW(None, IDI_APPLICATION).ok() }
}

/// COM нужен оболочке для меню трея.
pub fn ensure_com() -> bool {
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    if !initialized {
        program::log("COM уже инициализирован в другом режиме");
    }
    initialized
}

/// Завершает инициализацию COM, если она была выполнена.
pub fn release_com() {
    unsafe {
        CoUninitialize();
    }
}

/// Просматривает очередь сообщений, пока окно перерисовывается.
///
/// Это замена `Application.DoEvents` из оригинала: нужна после долгих
/// операций вроде перезапуска Проводника. Повторный вход в обработчики
/// запрещён флагом, чтобы долгая операция не запустила себя ещё раз.
pub fn pump_messages() {
    use std::cell::Cell;

    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, PM_REMOVE,
    };

    thread_local! {
        static PUMPING: Cell<bool> = const { Cell::new(false) };
    }

    // Вложенная прокачка ничего не даёт и может зациклиться.
    if PUMPING.with(|pumping| pumping.replace(true)) {
        return;
    }

    let mut message = MSG::default();
    loop {
        let has_message = unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() };
        if !has_message {
            break;
        }

        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    PUMPING.with(|pumping| pumping.set(false));
}

/// `true`, если сейчас выполняется прокачка сообщений.
pub fn is_pumping() -> bool {
    use std::cell::Cell;
    thread_local! {
        static PUMPING: Cell<bool> = const { Cell::new(false) };
    }
    PUMPING.with(|pumping| pumping.get())
}