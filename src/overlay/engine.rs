use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use image::{DynamicImage, GenericImageView, ImageReader};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect,
    InvalidateRect, MapWindowPoints, RedrawWindow, SetDIBitsToDevice,
    UpdateWindow, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    HDC, PAINTSTRUCT, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE,
    RDW_UPDATENOW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_SZ,
    REG_VALUE_TYPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows,
    FindWindowExW, FindWindowW, GetClassNameW, GetClientRect, GetMessageW, GetParent,
    GetSystemMetrics, IsWindow, LoadCursorW,
    PostMessageW, PostQuitMessage, RegisterClassExW, SendMessageTimeoutW,
    SetWindowLongPtrW, SetWindowPos, CS_HREDRAW, CS_VREDRAW,
    GWLP_USERDATA, HTTRANSPARENT, HWND_BOTTOM, IDC_ARROW, MSG, SMTO_NORMAL,
    SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SWP_SHOWWINDOW,
    WM_DESTROY, WM_DISPLAYCHANGE, WM_ERASEBKGND, WM_NCHITTEST, WM_PAINT,
    WM_USER, WNDCLASSEXW, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_VISIBLE,
};

use crate::logger::{log_message, log_paint_event, reset_paint_counter};
use crate::system::paths::get_system_transcoded_wallpaper_path;

const CLASS_NAME: PCWSTR = w!("DesktopOverlaySurface");
const WM_SET_IMAGE_PATH: u32 = WM_USER + 101;
const WM_RE_ATTACH: u32 = WM_USER + 102;
const WM_CLEAN_REMOVE: u32 = WM_USER + 103;

struct CachedBitmap {
    path: PathBuf,
    width: u32,
    height: u32,
    bgra_data: Vec<u8>,
}

struct WindowContext {
    current_image_path: Option<PathBuf>,
    cached_bitmap: Option<CachedBitmap>,
    host_hwnd: HWND,
}

pub struct OverlayManager {
    overlay_thread: Mutex<Option<JoinHandle<()>>>,
    overlay_hwnd: Arc<AtomicPtr<c_void>>,
    host_hwnd: Arc<AtomicPtr<c_void>>,
    active_path: Arc<Mutex<Option<PathBuf>>>,
    is_running: Arc<AtomicBool>,
}

impl OverlayManager {
    pub fn new() -> Arc<Self> {
        let mgr = Arc::new(Self {
            overlay_thread: Mutex::new(None),
            overlay_hwnd: Arc::new(AtomicPtr::new(std::ptr::null_mut())),
            host_hwnd: Arc::new(AtomicPtr::new(std::ptr::null_mut())),
            active_path: Arc::new(Mutex::new(None)),
            is_running: Arc::new(AtomicBool::new(true)),
        });

        let mgr_clone = Arc::clone(&mgr);
        let handle = thread::Builder::new()
            .name("OverlayWin32Thread".to_string())
            .spawn(move || {
                run_overlay_message_loop(mgr_clone);
            })
            .expect("Failed to start overlay thread");

        *mgr.overlay_thread.lock().unwrap() = Some(handle);

        // Start watchdog thread
        let watchdog_mgr = Arc::clone(&mgr);
        thread::Builder::new()
            .name("OverlayWatchdogThread".to_string())
            .spawn(move || {
                run_watchdog_loop(watchdog_mgr);
            })
            .expect("Failed to start watchdog thread");

        mgr
    }

    pub fn apply_image(&self, path: PathBuf) {
        *self.active_path.lock().unwrap() = Some(path.clone());
        reset_paint_counter();

        let hwnd_raw = self.overlay_hwnd.load(Ordering::SeqCst);
        if !hwnd_raw.is_null() {
            let hwnd = HWND(hwnd_raw);
            unsafe {
                let _ = PostMessageW(hwnd, WM_SET_IMAGE_PATH, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub fn force_reattach(&self) {
        let hwnd_raw = self.overlay_hwnd.load(Ordering::SeqCst);
        if !hwnd_raw.is_null() {
            let hwnd = HWND(hwnd_raw);
            unsafe {
                let _ = PostMessageW(hwnd, WM_RE_ATTACH, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub fn remove_overlay(&self) {
        *self.active_path.lock().unwrap() = None;
        let hwnd_raw = self.overlay_hwnd.load(Ordering::SeqCst);
        if !hwnd_raw.is_null() {
            let hwnd = HWND(hwnd_raw);
            unsafe {
                let _ = PostMessageW(hwnd, WM_CLEAN_REMOVE, WPARAM(0), LPARAM(0));
            }
        }
    }

    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        let hwnd_raw = self.overlay_hwnd.load(Ordering::SeqCst);
        if hwnd_raw.is_null() {
            return false;
        }
        unsafe { IsWindow(HWND(hwnd_raw)).as_bool() }
    }

    #[allow(dead_code)]
    pub fn get_active_path(&self) -> Option<PathBuf> {
        self.active_path.lock().unwrap().clone()
    }
}

pub fn find_wallpaper_worker_w() -> Option<HWND> {
    unsafe {
        let progman = match FindWindowW(w!("Progman"), None) {
            Ok(h) if !h.0.is_null() => h,
            _ => {
                let mut found = HWND::default();
                unsafe extern "system" fn enum_progman(hwnd: HWND, lparam: LPARAM) -> windows::Win32::Foundation::BOOL {
                    let mut class_name = [0u16; 256];
                    let len = GetClassNameW(hwnd, &mut class_name);
                    if len > 0 {
                        let name = String::from_utf16_lossy(&class_name[..len as usize]);
                        if name == "Progman" {
                            let out = lparam.0 as *mut HWND;
                            *out = hwnd;
                            return windows::Win32::Foundation::BOOL(0);
                        }
                    }
                    windows::Win32::Foundation::BOOL(1)
                }
                let _ = EnumWindows(Some(enum_progman), LPARAM(&mut found as *mut HWND as isize));
                found
            }
        };

        if progman.0.is_null() {
            log_message("Ошибка: Progman не найден");
            return None;
        }

        // Send 0x052C to split WorkerW hierarchy
        let mut result_lparam = 0;
        let _ = SendMessageTimeoutW(
            progman,
            0x052C,
            WPARAM(0x0000000D),
            LPARAM(1),
            SMTO_NORMAL,
            1000,
            Some(&mut result_lparam),
        );

        // Collect all WorkerW top-level windows in Z-order
        struct EnumData {
            workers: Vec<HWND>,
        }
        let mut data = EnumData { workers: Vec::new() };

        unsafe extern "system" fn enum_workers(hwnd: HWND, lparam: LPARAM) -> windows::Win32::Foundation::BOOL {
            let mut class_name = [0u16; 256];
            let len = GetClassNameW(hwnd, &mut class_name);
            if len > 0 {
                let name = String::from_utf16_lossy(&class_name[..len as usize]);
                if name == "WorkerW" {
                    let data = &mut *(lparam.0 as *mut EnumData);
                    data.workers.push(hwnd);
                }
            }
            windows::Win32::Foundation::BOOL(1)
        }

        let _ = EnumWindows(Some(enum_workers), LPARAM(&mut data as *mut EnumData as isize));

        // Locate WorkerW owning SHELLDLL_DefView
        let mut icons_worker_idx = None;
        for (i, &w) in data.workers.iter().enumerate() {
            let shell_view = FindWindowExW(w, None, w!("SHELLDLL_DefView"), None);
            if let Ok(sv) = shell_view {
                if !sv.0.is_null() {
                    icons_worker_idx = Some(i);
                    break;
                }
            }
        }

        if let Some(idx) = icons_worker_idx {
            if idx + 1 < data.workers.len() {
                let target = data.workers[idx + 1];
                log_message(&format!("Найден WorkerW хост: {:?} (следующий после иконок #{})", target.0, idx));
                return Some(target);
            } else {
                log_message("WorkerW после иконок отсутствует, fallback на Progman");
                return Some(progman);
            }
        }

        // Check if SHELLDLL_DefView is inside Progman
        let shell_view_progman = FindWindowExW(progman, None, w!("SHELLDLL_DefView"), None);
        if let Ok(sv) = shell_view_progman {
            if !sv.0.is_null() {
                if let Some(&first_worker) = data.workers.first() {
                    log_message(&format!("SHELLDLL_DefView в Progman, берем первый WorkerW: {:?}", first_worker.0));
                    return Some(first_worker);
                }
                return Some(progman);
            }
        }

        if let Some(&first_worker) = data.workers.first() {
            log_message(&format!("SHELLDLL_DefView не обнаружен, fallback на первый WorkerW: {:?}", first_worker.0));
            return Some(first_worker);
        }

        log_message("Fallback на Progman");
        Some(progman)
    }
}

fn get_system_wallpaper_path() -> Option<PathBuf> {
    if let Some(path) = get_system_transcoded_wallpaper_path() {
        return Some(path);
    }

    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Control Panel\\Desktop"),
            0,
            KEY_READ,
            &mut hkey,
        )
        .is_ok()
        {
            let mut buf = [0u16; 512];
            let mut data_type = REG_VALUE_TYPE(0);
            let mut data_size = (buf.len() * std::mem::size_of::<u16>()) as u32;

            let res = RegQueryValueExW(
                hkey,
                w!("WallPaper"),
                None,
                Some(&mut data_type),
                Some(buf.as_mut_ptr() as *mut u8),
                Some(&mut data_size),
            );
            let _ = RegCloseKey(hkey);

            if res.is_ok() && data_type == REG_SZ {
                let s = String::from_utf16_lossy(&buf)
                    .trim_matches('\0')
                    .trim()
                    .to_string();
                let p = PathBuf::from(s);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn cover_scale_image(
    img: &DynamicImage,
    client_w: u32,
    client_h: u32,
) -> Vec<u8> {
    let (img_w, img_h) = img.dimensions();
    if img_w == 0 || img_h == 0 || client_w == 0 || client_h == 0 {
        return vec![0; (client_w * client_h * 4) as usize];
    }

    let scale_x = client_w as f64 / img_w as f64;
    let scale_y = client_h as f64 / img_h as f64;
    let scale = scale_x.max(scale_y);

    let target_w = ((img_w as f64 * scale).round() as u32).max(1);
    let target_h = ((img_h as f64 * scale).round() as u32).max(1);

    let offset_x = if target_w > client_w { (target_w - client_w) / 2 } else { 0 };
    let offset_y = if target_h > client_h { (target_h - client_h) / 2 } else { 0 };

    let resized = img.resize_exact(target_w, target_h, image::imageops::FilterType::Triangle);
    let cropped = resized.crop_imm(offset_x, offset_y, client_w, client_h);

    let mut bgra = Vec::with_capacity((client_w * client_h * 4) as usize);
    let rgba = cropped.to_rgba8();

    for pixel in rgba.pixels() {
        // GDI expects BGRA
        bgra.push(pixel[2]); // B
        bgra.push(pixel[1]); // G
        bgra.push(pixel[0]); // R
        bgra.push(0xFF);     // A / Reserved
    }

    bgra
}

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    let ctx_ptr = ptr as *mut WindowContext;

    match msg {
        WM_NCHITTEST => {
            // Pass clicks through to desktop icons/shell
            LRESULT(HTTRANSPARENT as isize)
        }
        WM_ERASEBKGND => {
            // Suppress background erasing to avoid flicker
            LRESULT(1)
        }
        WM_DISPLAYCHANGE => {
            if !ctx_ptr.is_null() {
                let ctx = &mut *ctx_ptr;
                ctx.cached_bitmap = None;
                reposition_overlay(hwnd, ctx.host_hwnd);
            }
            let _ = InvalidateRect(hwnd, None, true);
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc: HDC = BeginPaint(hwnd, &mut ps);

            if !hdc.0.is_null() {
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                let client_w = (rc.right - rc.left).max(0) as u32;
                let client_h = (rc.bottom - rc.top).max(0) as u32;

                // Check OVERLAY_DEBUG_FILL=1
                let debug_fill = std::env::var("OVERLAY_DEBUG_FILL").map(|v| v == "1").unwrap_or(false);

                if debug_fill {
                    let brush = CreateSolidBrush(COLORREF(0x00FF00FF)); // Magenta
                    FillRect(hdc, &rc, brush);
                    let _ = DeleteObject(brush);
                    log_paint_event();
                } else if !ctx_ptr.is_null() && client_w > 0 && client_h > 0 {
                    let ctx = &mut *ctx_ptr;

                    if let Some(ref path) = ctx.current_image_path {
                        let need_render = match &ctx.cached_bitmap {
                            Some(cached) => {
                                cached.path != *path || cached.width != client_w || cached.height != client_h
                            }
                            None => true,
                        };

                        if need_render {
                            if let Ok(reader) = ImageReader::open(path) {
                                if let Ok(dyn_img) = reader.decode() {
                                    let bgra = cover_scale_image(&dyn_img, client_w, client_h);
                                    ctx.cached_bitmap = Some(CachedBitmap {
                                        path: path.clone(),
                                        width: client_w,
                                        height: client_h,
                                        bgra_data: bgra,
                                    });
                                }
                            }
                        }

                        if let Some(ref cached) = ctx.cached_bitmap {
                            let bmi = BITMAPINFO {
                                bmiHeader: BITMAPINFOHEADER {
                                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                                    biWidth: client_w as i32,
                                    biHeight: -(client_h as i32), // top-down
                                    biPlanes: 1,
                                    biBitCount: 32,
                                    biCompression: BI_RGB.0,
                                    biSizeImage: 0,
                                    biXPelsPerMeter: 0,
                                    biYPelsPerMeter: 0,
                                    biClrUsed: 0,
                                    biClrImportant: 0,
                                },
                                bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD::default()],
                            };

                            SetDIBitsToDevice(
                                hdc,
                                0,
                                0,
                                client_w,
                                client_h,
                                0,
                                0,
                                0,
                                client_h,
                                cached.bgra_data.as_ptr() as *const c_void,
                                &bmi,
                                DIB_RGB_COLORS,
                            );

                            log_paint_event();
                        }
                    }
                }

                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn reposition_overlay(hwnd: HWND, host_hwnd: HWND) {
    unsafe {
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);

        let mut pt = POINT { x: 0, y: 0 };
        MapWindowPoints(HWND::default(), host_hwnd, std::slice::from_mut(&mut pt));

        let _ = SetWindowPos(
            hwnd,
            HWND_BOTTOM,
            pt.x,
            pt.y,
            screen_w,
            screen_h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

fn create_overlay_window(host_hwnd: HWND, ctx: &mut WindowContext) -> Result<HWND, String> {
    unsafe {
        let hinstance = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandle failed: {}", e))?;

        let wnd_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(overlay_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance.into(),
            hIcon: windows::Win32::UI::WindowsAndMessaging::HICON::default(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(std::ptr::null_mut()),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: CLASS_NAME,
            hIconSm: windows::Win32::UI::WindowsAndMessaging::HICON::default(),
        };

        let _ = RegisterClassExW(&wnd_class);

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);

        let mut pt = POINT { x: 0, y: 0 };
        MapWindowPoints(HWND::default(), host_hwnd, std::slice::from_mut(&mut pt));

        let no_transparent = std::env::var("OVERLAY_NO_EX_TRANSPARENT").map(|v| v == "1").unwrap_or(false);
        let mut ex_style = WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
        if !no_transparent {
            ex_style |= WS_EX_TRANSPARENT;
        }

        let hwnd = CreateWindowExW(
            ex_style,
            CLASS_NAME,
            w!("DesktopOverlaySurfaceWindow"),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
            pt.x,
            pt.y,
            screen_w,
            screen_h,
            host_hwnd,
            None,
            hinstance,
            None,
        ).map_err(|e| format!("CreateWindowExW failed: {}", e))?;

        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ctx as *mut WindowContext as isize);

        let _ = SetWindowPos(
            hwnd,
            HWND_BOTTOM,
            pt.x,
            pt.y,
            screen_w,
            screen_h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );

        log_message(&format!(
            "Оверлей успешно создан: HWND={:?}, Host={:?}, Размер={}x{}",
            hwnd.0, host_hwnd.0, screen_w, screen_h
        ));

        Ok(hwnd)
    }
}

fn clean_removal_sequence(hwnd: HWND, ctx: &mut WindowContext) {
    unsafe {
        // 1. Repaint overlay with genuine system wallpaper if available
        if let Some(sys_wp) = get_system_wallpaper_path() {
            log_message(&format!("Восстановление системных обоев перед закрытием: {:?}", sys_wp));
            ctx.current_image_path = Some(sys_wp);
            ctx.cached_bitmap = None;
            let _ = InvalidateRect(hwnd, None, true);
            let _ = UpdateWindow(hwnd);
        }

        // 2. Sleep 150 ms to allow DWM presentation
        thread::sleep(Duration::from_millis(150));

        // 3. Destroy overlay window
        let _ = DestroyWindow(hwnd);

        // 4. Redraw Progman and all WorkerW
        let progman = FindWindowW(w!("Progman"), None);
        if let Ok(p) = progman {
            if !p.0.is_null() {
                let _ = RedrawWindow(
                    p,
                    None,
                    None,
                    RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW | RDW_FRAME,
                );
            }
        }

        unsafe extern "system" fn enum_refresh(hwnd: HWND, _lparam: LPARAM) -> windows::Win32::Foundation::BOOL {
            let mut class_name = [0u16; 256];
            let len = GetClassNameW(hwnd, &mut class_name);
            if len > 0 {
                let name = String::from_utf16_lossy(&class_name[..len as usize]);
                if name == "WorkerW" {
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
        let _ = EnumWindows(Some(enum_refresh), LPARAM(0));

        log_message("Оверлей чисто удален, рабочий стол перерисован");
    }
}

fn run_overlay_message_loop(mgr: Arc<OverlayManager>) {
    let mut host_hwnd = match find_wallpaper_worker_w() {
        Some(h) => h,
        None => {
            log_message("Не удалось найти окно хоста WorkerW / Progman");
            return;
        }
    };

    let mut ctx = WindowContext {
        current_image_path: mgr.active_path.lock().unwrap().clone(),
        cached_bitmap: None,
        host_hwnd,
    };

    let overlay_hwnd = match create_overlay_window(host_hwnd, &mut ctx) {
        Ok(h) => h,
        Err(e) => {
            log_message(&format!("Ошибка создания окна оверлея: {}", e));
            return;
        }
    };

    mgr.overlay_hwnd.store(overlay_hwnd.0, Ordering::SeqCst);
    mgr.host_hwnd.store(host_hwnd.0, Ordering::SeqCst);

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_SET_IMAGE_PATH {
                let path = mgr.active_path.lock().unwrap().clone();
                ctx.current_image_path = path;
                ctx.cached_bitmap = None;
                let _ = InvalidateRect(overlay_hwnd, None, true);
                let _ = UpdateWindow(overlay_hwnd);
            } else if msg.message == WM_RE_ATTACH {
                if let Some(new_host) = find_wallpaper_worker_w() {
                    host_hwnd = new_host;
                    ctx.host_hwnd = new_host;
                    mgr.host_hwnd.store(host_hwnd.0, Ordering::SeqCst);
                    reposition_overlay(overlay_hwnd, host_hwnd);
                    let _ = InvalidateRect(overlay_hwnd, None, true);
                    let _ = UpdateWindow(overlay_hwnd);
                    log_message("Принудительное переприкрепление оверлея выполнено");
                }
            } else if msg.message == WM_CLEAN_REMOVE {
                clean_removal_sequence(overlay_hwnd, &mut ctx);
                mgr.overlay_hwnd.store(std::ptr::null_mut(), Ordering::SeqCst);
                break;
            } else {
                let _ = DispatchMessageW(&msg);
            }
        }
    }

    mgr.overlay_hwnd.store(std::ptr::null_mut(), Ordering::SeqCst);
}

fn run_watchdog_loop(mgr: Arc<OverlayManager>) {
    let mut last_invalidate_tick = std::time::Instant::now();

    while mgr.is_running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(500));

        let hwnd_raw = mgr.overlay_hwnd.load(Ordering::SeqCst);
        let active_image = mgr.active_path.lock().unwrap().clone();

        if active_image.is_some() {
            let mut need_reattach = false;

            if hwnd_raw.is_null() {
                need_reattach = true;
            } else {
                let hwnd = HWND(hwnd_raw);
                unsafe {
                    if !IsWindow(hwnd).as_bool() {
                        need_reattach = true;
                    } else {
                        let current_parent = GetParent(hwnd);
                        let stored_host = HWND(mgr.host_hwnd.load(Ordering::SeqCst));
                        if current_parent != Ok(stored_host) {
                            need_reattach = true;
                        }
                    }
                }
            }

            if need_reattach {
                log_message("пересоздание оверлея: окно потеряно (перезапуск Explorer?)");
                // Stop previous overlay thread if needed and start a new one
                let mgr_clone = Arc::clone(&mgr);
                let handle = thread::Builder::new()
                    .name("OverlayWin32ThreadReborn".to_string())
                    .spawn(move || {
                        run_overlay_message_loop(mgr_clone);
                    });
                if let Ok(h) = handle {
                    *mgr.overlay_thread.lock().unwrap() = Some(h);
                }
            } else if !hwnd_raw.is_null() && last_invalidate_tick.elapsed() >= Duration::from_millis(1000) {
                // 1000 ms periodic InvalidateRect + UpdateWindow
                last_invalidate_tick = std::time::Instant::now();
                let hwnd = HWND(hwnd_raw);
                unsafe {
                    let _ = InvalidateRect(hwnd, None, false);
                    let _ = UpdateWindow(hwnd);
                }
            }
        }
    }
}
