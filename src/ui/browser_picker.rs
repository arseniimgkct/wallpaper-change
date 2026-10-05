//! Небольшой выбор: каким браузером заменить Edge.
//!
//! Edge открепляется от панели задач только после подтверждения.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW, IsWindow, LoadCursorW,
    RegisterClassExW, SetCursor, SetWindowLongPtrW, SetWindowPos, ShowWindow, CS_HREDRAW,
    CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA, IDCANCEL, IDC_HAND, MSG, SW_SHOW, SWP_NOSIZE,
    SWP_NOOWNERZORDER, WM_COMMAND, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_NCDESTROY, WM_PAINT, WM_QUIT, WM_SETCURSOR, WINDOW_EX_STYLE, WINDOW_STYLE,
    WS_EX_CONTROLPARENT, WS_EX_DLGMODALFRAME, WS_OVERLAPPED, WS_POPUP, WS_SYSMENU, WS_VISIBLE,
    WNDCLASSEXW,
};
use windows::core::PCWSTR;

use crate::browser::{self, BrowserInfo};
use crate::program::{self, utf16_with_nul};
use crate::ui::paint::{self, Painter};

const CLASS_NAME: &str = "DesktopOverlayBrowserPicker";

/// Логический размер окна выбора: как в оригинале, 430 x 210.
const WINDOW_WIDTH: i32 = 430;
const WINDOW_HEIGHT: i32 = 210;

/// Подпись под кнопками объясняет, чего ждать от каждого браузера.
const NOTE: &str = "Firefox назначит себя сам. Chrome и Edge откроют свою страницу настроек — \
                   там нужно нажать «Сделать стандартным». Edge при этом открепится от панели задач.";

/// Состояние окна выбора.
struct PickerState {
    hwnd: HWND,
    browsers: Vec<BrowserInfo>,
    /// Прямоугольники кнопок: сначала браузеры, последняя — «Отмена».
    buttons: Vec<RECT>,
    hover: Option<usize>,
    pressed: Option<usize>,
    /// Какой браузер выбран пользователем.
    selected: Option<usize>,
}

impl PickerState {
    fn button_label(&self, index: usize) -> String {
        format!("Сделать {} браузером по умолчанию", self.browsers[index].title)
    }

    fn hit(&self, x: i32, y: i32) -> Option<usize> {
        self.buttons.iter().position(|rect| paint::contains(*rect, x, y))
    }

    fn set_hover(&mut self, index: Option<usize>) {
        if self.hover != index {
            self.hover = index;
            unsafe {
                let _ = InvalidateRect(Some(self.hwnd), None, false);
            }
        }
    }

    /// Кнопка «Отмена» идёт последней.
    fn is_cancel(&self, index: usize) -> bool {
        index == self.buttons.len().saturating_sub(1)
    }
}

/// Показывает окно выбора и возвращает выбранный браузер.
pub fn show(owner: HWND, browsers: &[BrowserInfo]) -> Option<BrowserInfo> {
    if browsers.is_empty() {
        return None;
    }

    register_class();

    let title = utf16_with_nul(&format!("Браузер по умолчанию — {}", program::APP_NAME));
    let class = utf16_with_nul(CLASS_NAME);

    let state = Box::new(PickerState {
        hwnd: HWND::default(),
        browsers: browsers.to_vec(),
        buttons: layout_buttons(browsers.len()),
        hover: None,
        pressed: None,
        selected: None,
    });

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_DLGMODALFRAME.0 | WS_EX_CONTROLPARENT.0),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WINDOW_STYLE(WS_POPUP.0 | WS_OVERLAPPED.0 | WS_SYSMENU.0 | WS_VISIBLE.0),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
            Some(owner),
            None,
            GetModuleHandleW(None).ok().map(Into::into),
            None,
        )
    };

    let hwnd = match hwnd {
        Ok(hwnd) => hwnd,
        Err(e) => {
            program::log(format!("не удалось создать окно выбора браузера: {e}"));
            return None;
        }
    };

    unsafe {
        let mut state = state;
        state.hwnd = hwnd;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&mut *state as *mut PickerState) as isize);
        std::mem::forget(state);

        // Родитель блокируется: выбор браузера обязателен для этого действия.
        let _ = EnableWindow(owner, false);
        center_on(owner, hwnd);
        let _ = ShowWindow(hwnd, SW_SHOW);

        let closed = modal_loop(hwnd);

        let _ = EnableWindow(owner, true);

        if !closed {
            return None;
        }

        let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
        if pointer == 0 {
            return None;
        }

        let state = &mut *(pointer as *mut PickerState);
        state.selected.map(|index| state.browsers[index].clone())
    }
}

/// Раскладка кнопок окна выбора.
fn layout_buttons(count: usize) -> Vec<RECT> {
    let mut buttons = Vec::with_capacity(count + 1);
    let mut y = 70;

    for _ in 0..count {
        buttons.push(paint::rect(20, y, 390, 34));
        y += 40;
    }

    // «Отмена» прижата к низу окна.
    buttons.push(paint::rect(300, WINDOW_HEIGHT - 40, 110, 30));
    buttons
}

/// Отдельный цикл сообщений модального окна.
///
/// `true`, если окно закрылось пользователем, `false` — если приложение
/// завершилось извне.
unsafe fn modal_loop(hwnd: HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, TranslateMessage};

    let mut message = MSG::default();

    while GetMessageW(&mut message, None, 0, 0).as_bool() {
        if message.message == WM_QUIT {
            return false;
        }

        let _ = TranslateMessage(&message);
        DispatchMessageW(&message);

        // Окно уничтожено — выбор сделан.
        if !IsWindow(Some(hwnd)).as_bool() {
            return true;
        }
    }

    false
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

/// Состояние окна по `GWLP_USERDATA`.
unsafe fn state_mut(hwnd: HWND) -> Option<&'static mut PickerState> {
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if pointer == 0 {
        return None;
    }
    Some(&mut *(pointer as *mut PickerState))
}

fn center_on(owner: HWND, hwnd: HWND) {
    let Some(owner_rect) = program::window_rect(owner) else {
        return;
    };
    let Some(rect) = program::window_rect(hwnd) else {
        return;
    };

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    let x = owner_rect.left + (owner_rect.right - owner_rect.left - width) / 2;
    let y = owner_rect.top + (owner_rect.bottom - owner_rect.top - height) / 2;

    unsafe {
        let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOOWNERZORDER);
    }
}

pub(crate) unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_PAINT => {
            paint(hwnd);
            return LRESULT(0);
        }

        WM_ERASEBKGND => return LRESULT(1),

        WM_MOUSEMOVE => {
            let (x, y) = ((lparam.0 as u16) as i32, ((lparam.0 >> 16) as u16) as i32);
            if let Some(state) = state_mut(hwnd) {
                let hit = state.hit(x, y);
                state.set_hover(hit);
                if state.pressed.is_some() {
                    let _ = SetCapture(hwnd);
                }
            }
            return LRESULT(0);
        }

        WM_LBUTTONDOWN => {
            if let Some(state) = state_mut(hwnd) {
                let (x, y) = ((lparam.0 as u16) as i32, ((lparam.0 >> 16) as u16) as i32);
                state.pressed = state.hit(x, y);
                let _ = SetCapture(hwnd);
            }
            return LRESULT(0);
        }

        WM_LBUTTONUP => {
            let _ = ReleaseCapture();

            let (x, y) = ((lparam.0 as u16) as i32, ((lparam.0 >> 16) as u16) as i32);

            if let Some(state) = state_mut(hwnd) {
                let hit = state.hit(x, y);
                let pressed = state.pressed;
                state.pressed = None;

                // Срабатывает только если отпустили над той же кнопкой.
                if hit.is_some() && hit == pressed {
                    let index = hit.unwrap();
                    state.selected = if state.is_cancel(index) { None } else { Some(index) };
                    let _ = DestroyWindow(hwnd);
                }
            }

            return LRESULT(0);
        }

        // Курсор «рука» над всем окном: кликать тут только по кнопкам.
        WM_SETCURSOR => {
            if let Ok(cursor) = LoadCursorW(None, IDC_HAND) {
                let _ = SetCursor(Some(cursor));
            }
            return LRESULT(1);
        }

        WM_COMMAND => {
            // Escape закрывает окно без выбора.
            if wparam.0 as u32 == IDCANCEL.0 as u32 {
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }
        }

        WM_NCDESTROY => {
            let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if pointer != 0 {
                drop(Box::from_raw(pointer as *mut PickerState));
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }

        _ => {}
    }

    DefWindowProcW(hwnd, message, wparam, lparam)
}

unsafe fn paint(hwnd: HWND) {
    let Some(state) = state_mut(hwnd) else {
        return;
    };

    let mut paint_struct = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut paint_struct);
    if hdc.is_invalid() {
        return;
    }

    let painter = Painter::new(hdc, 96);
    painter.fill(paint::rect(0, 0, WINDOW_WIDTH, WINDOW_HEIGHT), paint::BACKGROUND);

    painter.label_bold(
        paint::rect(20, 16, 390, 24),
        "Кем заменить Microsoft Edge?",
        10.0,
        paint::rgb(235, 235, 235),
    );

    let current = format!("Сейчас по умолчанию: {}", browser::current_title());
    painter.label(
        paint::rect(20, 42, 390, 22),
        &current,
        8.5,
        paint::rgb(150, 150, 150),
    );

    // Первый браузер в списке показываем как основной вариант.
    for (index, rect) in state.buttons.iter().enumerate() {
        let is_cancel = state.is_cancel(index);
        let label = if is_cancel {
            "Отмена".to_string()
        } else {
            state.button_label(index)
        };

        painter.button(
            *rect,
            &label,
            index == 0,
            state.hover == Some(index),
            state.pressed == Some(index),
            true,
            9.0,
            false,
        );
    }

    let note_top = 70 + state.browsers.len() as i32 * 40;
    painter.label(paint::rect(20, note_top, 390, 40), NOTE, 8.5, paint::rgb(125, 125, 125));

    let _ = EndPaint(hwnd, &paint_struct);
}