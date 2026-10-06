use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;
use image::{DynamicImage, RgbaImage};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use crate::logger::log_message;
use crate::system::wallpaper_store::{is_supported_image, save_dynamic_image, save_image_copy};

const CF_HDROP_ID: u32 = 15;

fn scan_dir_for_first_image(dir: &Path) -> Option<PathBuf> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && is_supported_image(p))
            .collect();
        files.sort_by(|a, b| {
            a.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase()
                .cmp(&b.file_name().unwrap_or_default().to_string_lossy().to_lowercase())
        });
        return files.into_iter().next();
    }
    None
}

pub fn try_read_clipboard_hdrop() -> Option<PathBuf> {
    unsafe {
        if IsClipboardFormatAvailable(CF_HDROP_ID).is_err() {
            return None;
        }
        if OpenClipboard(HWND::default()).is_err() {
            return None;
        }

        let handle: HANDLE = match GetClipboardData(CF_HDROP_ID) {
            Ok(h) => h,
            Err(_) => {
                let _ = CloseClipboard();
                return None;
            }
        };

        let hdrop = HDROP(handle.0);
        let count = DragQueryFileW(hdrop, 0xFFFFFFFF, None);
        let mut result_path: Option<PathBuf> = None;

        if count > 0 {
            let mut buf = [0u16; 1024];
            let len = DragQueryFileW(hdrop, 0, Some(&mut buf));
            if len > 0 {
                let s = String::from_utf16_lossy(&buf[..len as usize]);
                let p = PathBuf::from(s);
                if p.is_file() && is_supported_image(&p) {
                    result_path = Some(p);
                } else if p.is_dir() {
                    result_path = scan_dir_for_first_image(&p);
                }
            }
        }

        let _ = CloseClipboard();
        result_path
    }
}

pub fn read_clipboard_with_retry() -> Result<PathBuf, String> {
    const MAX_RETRIES: usize = 12;
    const DELAY_MS: u64 = 80;

    for attempt in 1..=MAX_RETRIES {
        // Priority 1: CF_HDROP
        if let Some(path) = try_read_clipboard_hdrop() {
            log_message(&format!("Буфер обмена (CF_HDROP): найдено изображение {:?}", path));
            return save_image_copy(&path);
        }

        // Priority 2: Text file path
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            if let Ok(text) = clipboard.get_text() {
                let text_trimmed = text.trim().trim_matches('"').trim();
                let path = PathBuf::from(text_trimmed);
                if path.is_file() && is_supported_image(&path) {
                    log_message(&format!("Буфер обмена (текстовый путь): найдено изображение {:?}", path));
                    return save_image_copy(&path);
                } else if path.is_dir() {
                    if let Some(first_img) = scan_dir_for_first_image(&path) {
                        log_message(&format!("Буфер обмена (папка): найдено изображение {:?}", first_img));
                        return save_image_copy(&first_img);
                    }
                }
            }

            // Priority 3: Direct bitmap image
            if let Ok(img_data) = clipboard.get_image() {
                let w = img_data.width as u32;
                let h = img_data.height as u32;
                let bytes = img_data.bytes.into_owned();
                if let Some(rgba_img) = RgbaImage::from_raw(w, h, bytes) {
                    let dyn_img = DynamicImage::ImageRgba8(rgba_img);
                    log_message(&format!("Буфер обмена (растр): получено изображение {}x{}", w, h));
                    return save_dynamic_image(&dyn_img);
                }
            }
        }

        if attempt < MAX_RETRIES {
            sleep(Duration::from_millis(DELAY_MS));
        }
    }

    Err("В буфере обмена не найдено поддерживаемое изображение или путь к файлу".to_string())
}
