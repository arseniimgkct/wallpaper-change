use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetWindowLongPtrW, RegisterClassExW,
    SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, CS_HREDRAW, CS_VREDRAW,
    GWLP_USERDATA, HWND_TOPMOST, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW, WM_NCCREATE, WM_PAINT,
    WINDOW_EX_STYLE, WINDOW_STYLE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
    WNDCLASSEXW,
};
use windows::core::PCWSTR;

use crate::native;
use crate::program::{self, utf16_with_nul};
use crate::ui::paint::{self, Painter};

const CLASS_NAME: &str = "DesktopOverlayTooltip";

pub const SHOW_DELAY_MS: u32 = 500;

struct TooltipState {
    text: String,
    /// Окно утилиты: по нему пересчитываются координаты подсказки.
    owner: HWND,
}

impl TooltipState {
    fn measure(text: &str) -> (i32, i32) {
        let longest = text.lines().map(str::len).max().unwrap_or(8);
        let width = (longest as i32 * 7 + 24).clamp(80, 460);
        let height = text.lines().count() as i32 * 16 + 18;
        (width, height)
    }
}

pub struct Tooltip {
    hwnd: HWND,
}

impl Tooltip {
    pub fn create(owner: HWND) -> Tooltip {
        register_class();

        let class = utf16_with_nul(CLASS_NAME);
        let mut state = Box::new(TooltipState { text: String::new(), owner });

        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0 | WS_EX_TRANSPARENT.0),
                PCWSTR(class.as_ptr()),
                PCWSTR::null(),
                WINDOW_STYLE(WS_POPUP.0),
                0,
                0,
                100,
                30,
                None,
                None,
                GetModuleHandleW(None).ok().map(Into::into),
                None,
            );

            let hwnd = hwnd.unwrap_or_default();
            if hwnd.0.is_null() {
                program::log("не удалось создать окно подсказки");
                return Tooltip { hwnd };
            }

            native::enable_dark_mode_for_window(hwnd);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&mut *state as *mut TooltipState) as isize);
            std::mem::forget(state);

            Tooltip { hwnd }
        }
    }

    pub fn show_below(&self, text: &str, mut anchor: RECT) {
        if self.hwnd.0.is_null() || text.is_empty() {
            return;
        }

        let (width, height) = TooltipState::measure(text);

        unsafe {
            let pointer = GetWindowLongPtrW(self.hwnd, GWLP_USERDATA);
            if pointer != 0 {
                let state = &mut *(pointer as *mut TooltipState);
                state.text = text.to_string();

                // Прямоугольник элемента задан в координатах клиентской
                // области окна утилиты, а подсказка живёт в экранных: без
                // пересчёта она уехала бы в угол экрана.
                anchor = client_to_screen(state.owner, anchor);
            }

            let title = utf16_with_nul(text);
            let _ = SetWindowTextW(self.hwnd, PCWSTR(title.as_ptr()));

            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                anchor.left + (anchor.right - anchor.left - width) / 2,
                anchor.bottom + 6,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }

    pub fn hide(&self) {
        if self.hwnd.0.is_null() {
            return;
        }
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
}

/// Переводит прямоугольник клиентской области `owner` в экранные координаты.
fn client_to_screen(owner: HWND, rect: RECT) -> RECT {
    use windows::Win32::Graphics::Gdi::ClientToScreen;

    let mut origin = POINT { x: rect.left, y: rect.top };
    unsafe {
        let _ = ClientToScreen(owner, &mut origin);
    }

    RECT {
        left: origin.x,
        right: origin.x + (rect.right - rect.left),
        top: origin.y,
        bottom: origin.y + (rect.bottom - rect.top),
    }
}

fn register_class() {
    let class = utf16_with_nul(CLASS_NAME);

    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCCREATE => {
            native::enable_dark_mode_for_window(hwnd);
            DefWindowProcW(hwnd, message, wparam, lparam)
        }

        WM_PAINT => {
            paint_tooltip(hwnd);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn paint_tooltip(hwnd: HWND) {
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if pointer == 0 {
        return;
    }

    let text = (*(pointer as *const TooltipState)).text.clone();

    let mut paint_struct = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut paint_struct);
    if hdc.is_invalid() {
        return;
    }

    let mut client = RECT::default();
    let _ = GetClientRect(hwnd, &mut client);
    let painter = Painter::new(hdc, 96);

    painter.tooltip_frame(client);
    painter.label(
        RECT { left: client.left + 6, right: client.right - 6, ..client },
        &text,
        8.5,
        paint::rgb(235, 235, 235),
    );

    let _ = EndPaint(hwnd, &paint_struct);
}

