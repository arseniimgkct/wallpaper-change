//! Окно оверлея: картинка, встроенная в рабочий стол Windows.
//!
//! Окно создаётся как дочернее для `WorkerW`, который отвечает за слой обоев,
//! поэтому картинка оказывается под значками и панелью задач. Оно неактивное и
//! прозрачное для мыши: клики должны доходить до рабочего стола.

use image::DynamicImage;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, InvalidateRect, SetDIBitsToDevice, UpdateWindow, BITMAPINFO,
    BITMAPINFOHEADER, DIB_RGB_COLORS, HDC, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetParent, GetWindowLongPtrW,
    IsWindow, RegisterClassExW, SetWindowLongPtrW, SetWindowPos,
    CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HWND_BOTTOM, SWP_NOACTIVATE, SWP_SHOWWINDOW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_DISPLAYCHANGE, WM_ERASEBKGND, WM_NCDESTROY,
    WM_NCHITTEST, WM_PAINT, WS_CHILD, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
    WS_VISIBLE, WNDCLASSEXW,
};
use windows::core::PCWSTR;

use crate::images;
use crate::native;
use crate::program::{self, utf16_with_nul};

const CLASS_NAME: &str = "DesktopOverlaySurface";
const WINDOW_TITLE: &str = "DesktopOverlay";

/// `HTTRANSPARENT`: клик должен пройти насквозь, на рабочий стол.
const HTTRANSPARENT: LRESULT = LRESULT(-1);

/// Максимум строк в лог отрисовки — чтобы лог не рос бесконечно.
const MAX_PAINT_LOGS: u32 = 8;

/// Состояние окна оверлея. Лежит в `GWLP_USERDATA`.
struct OverlayState {
    /// Окно и его родитель-рабочий стол: нужны обработчику сообщений.
    hwnd: HWND,
    host: HWND,
    /// Картинка пользователя.
    image: DynamicImage,
    /// Кэш: картинка, уже вписанная в окно.
    scaled: Option<images::Wallpaper>,
    /// Системные обои — показываются, когда оверлей снимают.
    system_wallpaper: Option<DynamicImage>,
    /// Показывать системные обои вместо пользовательской картинки.
    show_system_wallpaper: bool,
    /// Заливать окно маджентой: быстрый способ проверить, что оверлей на месте.
    debug_fill: bool,
    paint_log_count: u32,
}

impl OverlayState {
    fn is_alive(&self) -> bool {
        unsafe { IsWindow(Some(self.hwnd)).as_bool() }
    }

    /// Картинка, которую видно прямо сейчас.
    fn active_image(&self) -> &DynamicImage {
        match (&self.show_system_wallpaper, &self.system_wallpaper) {
            (true, Some(system)) => system,
            _ => &self.image,
        }
    }

    /// Растягивает окно на весь основной монитор.
    fn layout(&self) {
        if !self.is_alive() || self.host.0.is_null() {
            return;
        }

        unsafe {
            if !IsWindow(Some(self.host)).as_bool() {
                return;
            }
        }

        let screen = program::primary_screen_rect();
        let origin = native::screen_to_client(self.host, screen.left, screen.top);

        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_BOTTOM),
                origin.x,
                origin.y,
                screen.right - screen.left,
                screen.bottom - screen.top,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }

        if let Some(rect) = native::get_window_rect(self.hwnd) {
            program::log(&format!(
                "размещено: client=({},{}) {}x{}, экран=({},{}) {}x{}",
                origin.x,
                origin.y,
                screen.right - screen.left,
                screen.bottom - screen.top,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top
            ));
        }
    }

    /// Требует перерисовать окно прямо сейчас.
    fn force_repaint(&self) {
        if !self.is_alive() {
            return;
        }

        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, true);
            let _ = UpdateWindow(self.hwnd);
        }
    }

    fn log_paint(&mut self, message: String) {
        self.paint_log_count += 1;
        if self.paint_log_count > MAX_PAINT_LOGS {
            return;
        }
        program::log(&format!("[paint #{}] {}", self.paint_log_count, message));
    }
}

/// Окно оверлея снаружи: создание, перемещение и отрисовка.
pub struct OverlayWindow {
    hwnd: HWND,
    host: HWND,
    /// Состояние живёт, пока живёт окно, и умирает вместе с ним.
    state: Box<OverlayState>,
}

impl OverlayWindow {
    /// Создаёт окно и встраивает его в рабочий стол. `false` — не вышло.
    pub fn new(image: DynamicImage) -> Option<OverlayWindow> {
        // Окно не должно получать фокус, быть в панели задач и перехватывать мышь.
        let mut ex_style =
            WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | if transparent() { WS_EX_TRANSPARENT.0 } else { 0 };
        if ex_style == 0 {
            ex_style = WS_EX_NOACTIVATE.0;
        }

        let host = native::find_wallpaper_worker_w()?;
        if unsafe { !IsWindow(Some(host)).as_bool() } {
            return None;
        }

        ensure_class_registered();

        let state = Box::new(OverlayState {
            hwnd: HWND::default(),
            host,
            image,
            scaled: None,
            system_wallpaper: None,
            show_system_wallpaper: false,
            debug_fill: std::env::var("OVERLAY_DEBUG_FILL").as_deref() == Ok("1"),
            paint_log_count: 0,
        });

        let screen = program::primary_screen_rect();
        let origin = native::screen_to_client(host, screen.left, screen.top);

        let class = utf16_with_nul(CLASS_NAME);
        let title = utf16_with_nul(WINDOW_TITLE);

        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(ex_style),
                PCWSTR(class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
                origin.x,
                origin.y,
                screen.right - screen.left,
                screen.bottom - screen.top,
                Some(host),
                None,
                GetModuleHandleW(None).ok().map(Into::into),
                None,
            )
        };

        let mut state = state;
        let hwnd = match hwnd {
            Ok(hwnd) => hwnd,
            Err(e) => {
                program::log(&format!("CreateWindowEx вернул ошибку: {e}"));
                return None;
            }
        };

        state.hwnd = hwnd;
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&mut *state as *mut OverlayState) as isize);
        }

        let window = OverlayWindow { hwnd, host, state };

        window.state.layout();
        window.state.force_repaint();

        program::log(&format!(
            "прикреплено: hwnd={} host={}",
            hwnd.0 as usize,
            host.0 as usize
        ));

        Some(window)
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn host(&self) -> HWND {
        self.host
    }

    /// Окно всё ещё существует.
    pub fn is_alive(&self) -> bool {
        unsafe { IsWindow(Some(self.hwnd)).as_bool() }
    }

    /// Окно всё ещё дочернее для того же самого окна рабочего стола.
    pub fn is_still_attached(&self) -> bool {
        if !self.is_alive() || self.host.0.is_null() {
            return false;
        }

        unsafe {
            IsWindow(Some(self.host)).as_bool() && GetParent(self.hwnd).ok() == Some(self.host)
        }
    }

    /// Растягивает окно на весь основной монитор.
    pub fn layout_to_primary_monitor(&self) {
        self.state.layout();
    }

    /// Требует перерисовать окно прямо сейчас.
    pub fn force_repaint(&self) {
        self.state.force_repaint();
    }

    /// Показывает системные обои вместо картинки пользователя.
    ///
    /// Нужно в момент снятия оверлея: если просто закрыть окно, под ним
    /// останется серая «дыра» на месте обоев.
    pub fn show_system_wallpaper(&mut self) -> bool {
        if !self.is_alive() {
            return false;
        }

        if self.state.system_wallpaper.is_none() {
            self.state.system_wallpaper = load_system_wallpaper();
        }

        if self.state.system_wallpaper.is_none() {
            program::log("системные обои не прочитаны, окно будет просто закрыто");
            return false;
        }

        self.state.show_system_wallpaper = true;
        // Кэш ос��ался для другой картинки.
        self.state.scaled = None;
        self.force_repaint();
        true
    }
}

impl Drop for OverlayWindow {
    fn drop(&mut self) {
        if self.hwnd.0.is_null() {
            return;
        }

        unsafe {
            if IsWindow(Some(self.hwnd)).as_bool() {
                let _ = DestroyWindow(self.hwnd);
            }
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
        }

        self.hwnd = HWND::default();
        self.host = HWND::default();
    }
}

/// Прозрачность для мыши отключается переменной окружения — так удобно отлаживать.
fn transparent() -> bool {
    std::env::var("OVERLAY_NO_EX_TRANSPARENT").as_deref() != Ok("1")
}

/// Регистрирует класс окна. Повторные вызовы безопасны.
fn ensure_class_registered() {
    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        overlay_wnd_proc(hwnd, message, wparam, lparam)
    }

    unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(std::ptr::null_mut()),
            lpszClassName: PCWSTR(utf16_with_nul(CLASS_NAME).as_ptr()),
            ..Default::default()
        };

        // Повторная регистрация возвращает 0 — это не ошибка.
        RegisterClassExW(&class);
    }
}

/// Обработчик сообщений окна оверлея.
unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // Клик должен доходить до рабочего стола.
        WM_NCHITTEST => return HTTRANSPARENT,

        // Фон не перерисовываем: всё рисуется целиком в WM_PAINT.
        WM_ERASEBKGND => return LRESULT(1),

        WM_PAINT => {
            paint(hwnd);
            return LRESULT(0);
        }

        WM_DISPLAYCHANGE => {
            relayout(hwnd);
            repaint(hwnd);
            return LRESULT(0);
        }

        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            return LRESULT(0);
        }

        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }

        _ => {}
    }

    DefWindowProcW(hwnd, message, wparam, lparam)
}

/// Состояние окна по `GWLP_USERDATA`.
unsafe fn state(hwnd: HWND) -> Option<&'static mut OverlayState> {
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if pointer == 0 {
        return None;
    }
    Some(&mut *(pointer as *mut OverlayState))
}

/// Перерисовывает окно оверлея.
unsafe fn paint(hwnd: HWND) {
    let mut paint_struct = PAINTSTRUCT::default();
    let hdc: HDC = BeginPaint(hwnd, &mut paint_struct);
    if hdc.is_invalid() {
        program::log("BeginPaint вернул 0");
        return;
    }

    let mut client = windows::Win32::Foundation::RECT::default();
    let _ = GetClientRect(hwnd, &mut client);
    let (width, height) = (client.right - client.left, client.bottom - client.top);

    if let Some(state) = state(hwnd) {
        if state.debug_fill {
            state.log_paint(format!("DEBUG magenta, client={width}x{height}"));
            fill_magenta(hdc, width, height);
            let _ = EndPaint(hwnd, &paint_struct);
            return;
        }

        let source = state.active_image().clone();
        let key = (width, height);
        let needs_scale = match &state.scaled {
            Some(scaled) => (scaled.width, scaled.height) != key,
            None => true,
        };

        if needs_scale {
            state.scaled = images::Wallpaper::scaled_to(&source, width, height);
        }

        if let Some(scaled) = &state.scaled {
            blit(hdc, scaled);
            state.log_paint(format!(
                "paint client={width}x{height} rcPaint={},{} {}x{}",
                paint_struct.rcPaint.left,
                paint_struct.rcPaint.top,
                paint_struct.rcPaint.right - paint_struct.rcPaint.left,
                paint_struct.rcPaint.bottom - paint_struct.rcPaint.top
            ));
        }
    }

    let _ = EndPaint(hwnd, &paint_struct);
}

/// Копирует подготовленную картинку на окно.
unsafe fn blit(hdc: HDC, scaled: &images::Wallpaper) {
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: scaled.width,
            // Отрицательная высота: строки сверху вниз, без переворота.
            biHeight: -scaled.height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0,
            ..Default::default()
        },
        ..Default::default()
    };

    SetDIBitsToDevice(
        hdc,
        0,
        0,
        scaled.width as u32,
        scaled.height as u32,
        0,
        0,
        0,
        scaled.height as u32,
        scaled.bgra.as_ptr().cast(),
        &info,
        DIB_RGB_COLORS,
    );
}

/// Заливает окно маджентой для отладки.
unsafe fn fill_magenta(hdc: HDC, width: i32, height: i32) {
    let buffer = vec![255u8; (width * height * 4) as usize];
    blit(
        hdc,
        &images::Wallpaper { bgra: buffer, width, height },
    );
}

/// Перемещает окно на весь основной монитор.
unsafe fn relayout(hwnd: HWND) {
    if let Some(state) = state(hwnd) {
        state.layout();
    }
}

/// Требует перерисовки.
unsafe fn repaint(hwnd: HWND) {
    if let Some(state) = state(hwnd) {
        state.force_repaint();
    }
}

/// Системные обои: транскодированная копия и, если есть, путь из политики.
fn load_system_wallpaper() -> Option<DynamicImage> {
    let mut candidates = vec![program::appdata()
        .join("Microsoft")
        .join("Windows")
        .join("Themes")
        .join("TranscodedWallpaper")];

    if let Some(policy) = program::get_registry_string(
        r"Software\Microsoft\Windows\CurrentVersion\Policies\System",
        "Wallpaper",
    ) {
        candidates.push(std::path::PathBuf::from(policy));
    }

    for path in candidates {
        match images::load(&path) {
            Ok(image) => return Some(image),
            Err(error) => {
                program::log(&format!("не прочитаны системные обои {}: {error}", path.display()));
            }
        }
    }

    None
}
