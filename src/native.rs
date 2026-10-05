//! Обёртки над Win32, которые использовались из C# без посредников.

use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    RedrawWindow, REDRAW_WINDOW_FLAGS, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE,
    RDW_UPDATENOW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, SendMessageTimeoutW,
    SMTO_ABORTIFHUNG,
};
use windows::core::{BOOL, PCWSTR};

use crate::program::{self, utf16_with_nul};

/// Окно, внутри которого живут обои рабочего стола.
const PROGMAN: &str = "Progman";
const WORKERW: &str = "WorkerW";
const SHELLDLL_DEFVIEW: &str = "SHELLDLL_DefView";

/// Сообщение оболочке, заставляющее её пересоздать окна `WorkerW`.
const MSG_SPLIT_WORKERW_VALUE: u32 = 0x052C;

/// Находит окно, к которому нужно прикрепить оверлей.
///
/// Схема та же, что в оригинале: `Progman` просим пересоздать `WorkerW`,
/// затем ищем окно, у которого есть дочерний `SHELLDLL_DefView` (в нём живут
/// ярлыки), и берём следующее за ним окно — это и есть слой обоев.
pub fn find_wallpaper_worker_w() -> Option<HWND> {
    unsafe {
        let progman = FindWindowW(PCWSTR(utf16_with_nul(PROGMAN).as_ptr()), None).unwrap_or_default();

        let progman = if progman.is_invalid() {
            program::find_top_level_by_class(PROGMAN).unwrap_or_default()
        } else {
            progman
        };

        if !progman.is_invalid() {
            let _ = SendMessageTimeoutW(
                progman,
                MSG_SPLIT_WORKERW_VALUE,
                WPARAM(0),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                1000,
                None,
            );
        }

        let workers = top_level_windows_by_class(WORKERW);
        let icons_index = workers.iter().position(|w| has_shell_view(*w));

        match icons_index {
            Some(index) if index + 1 < workers.len() => Some(workers[index + 1]),
            Some(_) if !progman.is_invalid() => Some(progman),
            _ => None,
        }
    }
}

/// Есть ли у окна дочерний `SHELLDLL_DefView` — признак окна со значками.
fn has_shell_view(window: HWND) -> bool {
    unsafe {
        FindWindowExW(
            Some(window),
            None,
            PCWSTR(utf16_with_nul(SHELLDLL_DEFVIEW).as_ptr()),
            None,
        )
        .is_ok()
    }
}

/// Все верхнеуровневые окна с указанным классом, в порядке перечисления z-порядка.
pub fn top_level_windows_by_class(name: &str) -> Vec<HWND> {
    struct Ctx<'a> {
        name: &'a str,
        found: Vec<HWND>,
    }

    unsafe extern "system" fn callback(hwnd: HWND, data: LPARAM) -> BOOL {
        let ctx = &mut *(data.0 as *mut Ctx<'_>);
        if class_name(hwnd) == ctx.name {
            ctx.found.push(hwnd);
        }
        true.into()
    }

    let mut ctx = Ctx { name, found: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(callback), LPARAM(std::ptr::addr_of_mut!(ctx) as isize));
    }
    ctx.found
}

/// Ищет верхнее окно по имени класса. `None`, если такого окна нет.
pub fn find_top_level_by_class_if_exists(name: &str) -> bool {
    program::find_top_level_by_class(name).is_some()
}

/// Полный перерисовка рабочего стола: без неё изменения панели задач и
/// обоев остаются незамеченными до следующего события мыши.
pub fn redraw_desktop() {
    let flags = REDRAW_WINDOW_FLAGS(
        RDW_INVALIDATE.0 | RDW_ERASE.0 | RDW_ALLCHILDREN.0 | RDW_UPDATENOW.0 | RDW_FRAME.0,
    );

    unsafe {
        if let Some(progman) = program::find_top_level_by_class(PROGMAN) {
            let _ = RedrawWindow(Some(progman), None, None, flags);
        }

        for window in top_level_windows_by_class(WORKERW) {
            let _ = RedrawWindow(Some(window), None, None, flags);
        }
    }
}

/// Имя класса окна.
pub fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..len as usize])
}

/// Прямоугольник окна в экранных координатах.
pub fn get_window_rect(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect).ok()? };
    Some(rect)
}

/// Размер клиентской области окна.
pub fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut rect = RECT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rect);
    }
    (rect.right - rect.left, rect.bottom - rect.top)
}

/// Включает тёмное оформление заголовка окна (DWM).
pub fn enable_dark_mode_for_window(hwnd: HWND) {
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWINDOWATTRIBUTE,
    };

    /// `DWMWA_USE_IMMERSIVE_DARK_MODE` в новых сборках Windows и
    /// `DWMWA_USE_IMMERSIVE_DARK_MODE_OLD` в Windows 10 1809-1903.
    const USE_IMMERSIVE_DARK_MODE: i32 = 20;
    const USE_IMMERSIVE_DARK_MODE_OLD: i32 = 19;

    let on = 1i32;
    unsafe {
        for attribute in [USE_IMMERSIVE_DARK_MODE, USE_IMMERSIVE_DARK_MODE_OLD] {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(attribute as i32),
                (&on as *const i32).cast(),
                std::mem::size_of::<i32>() as u32,
            );
        }
    }
}

/// Переводит экранные координаты в клиентские координаты окна `host`.
pub fn screen_to_client(host: HWND, x: i32, y: i32) -> POINT {
    program::screen_to_client(host, x, y)
}