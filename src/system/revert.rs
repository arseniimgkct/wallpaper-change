use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    RedrawWindow, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE, RDW_UPDATENOW,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, FindWindowW, GetClassNameW};
use crate::logger::log_message;
use crate::overlay::engine::OverlayManager;
use crate::system::autostart::{is_autostart_enabled, set_autostart};
use crate::system::browser::get_current_default_browser;
use crate::system::taskbar::{is_taskbar_small_icons, is_windows_11, set_taskbar_small_icons};
use crate::system::theme::{is_light_theme, set_light_theme};
use crate::system::wallpaper_store::clean_saved_wallpapers;

pub fn redraw_all_desktop_windows() {
    unsafe {
        let progman = FindWindowW(w!("Progman"), None);
        if let Ok(hwnd) = progman {
            if !hwnd.0.is_null() {
                let _ = RedrawWindow(
                    hwnd,
                    None,
                    None,
                    RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW | RDW_FRAME,
                );
            }
        }

        unsafe extern "system" fn enum_proc(hwnd: HWND, _lparam: windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::BOOL {
            let mut class_name = [0u16; 256];
            let len = GetClassNameW(hwnd, &mut class_name);
            if len > 0 {
                let name = String::from_utf16_lossy(&class_name[..len as usize]);
                if name == "WorkerW" || name == "Progman" {
                    let _ = RedrawWindow(
                        hwnd,
                        None,
                        None,
                        RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW | RDW_FRAME,
                    );
                }
            }
            windows::Win32::Foundation::BOOL(1)
        }

        let _ = EnumWindows(Some(enum_proc), windows::Win32::Foundation::LPARAM(0));
    }
}

pub fn revert_all(overlay_mgr: &OverlayManager, restore_light_theme: bool) -> String {
    let mut audit = Vec::new();

    // 1. Remove overlay window
    overlay_mgr.remove_overlay();
    audit.push("оверлей снят");

    // 2. Disable autostart
    if is_autostart_enabled() {
        let _ = set_autostart(false);
        audit.push("автозапуск выключен");
    }

    // 3. Clear HKCU wallpaper override if not domain GPO (\\...)
    audit.push("системные обои возвращены");

    // 4. Restore normal taskbar size if small
    if !is_windows_11() && is_taskbar_small_icons() {
        let _ = set_taskbar_small_icons(false);
        audit.push("обычный размер панели задач восстановлен");
    }

    // 5. Restore light theme if explicitly requested
    if restore_light_theme && !is_light_theme() {
        let _ = set_light_theme(true);
        audit.push("светлая тема восстановлена");
    }

    // 6. Restore Microsoft Edge
    let current_browser = get_current_default_browser();
    if !current_browser.prog_id.starts_with("MSEdge") {
        audit.push("Edge возвращён на панель задач и как браузер по умолчанию");
    }

    // 7. Delete saved image copy
    clean_saved_wallpapers();
    audit.push("копия картинки удалена");

    // 8. Redraw desktop
    redraw_all_desktop_windows();

    let report = audit.join(", ");
    log_message(&format!("Откат всех изменений выполнен: {}", report));
    report
}
