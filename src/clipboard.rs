//! Картинка из буфера обмена.
//!
//! Поддерживаются те же случаи, что и в оригинале: файлы и папки из обмена,
//! путь в тексте и, наконец, само изображение (`CF_DIB`, `CF_DIBV5`, `CF_BITMAP`).

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use image::DynamicImage;
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, DIB_RGB_COLORS, HBITMAP, HDC,
};
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::{
    CF_BITMAP, CF_DIB, CF_DIBV5, CF_HDROP, OleInitialize, OleUninitialize,
};

use crate::images;
use crate::program;

/// Буфер обмена бывает занят другим приложением — пробуем несколько раз.
const ATTEMPTS: usize = 12;
const RETRY_DELAY: Duration = Duration::from_millis(80);

/// `CF_UNICODETEXT` в обмен не входит в crate, но значение фиксировано.
const CF_UNICODETEXT: u32 = 13;

/// Берёт картинку из буфера и возвращает путь к файлу.
///
/// `Err` содержит текст, который показывается пользователю как есть.
pub fn try_take() -> Result<PathBuf, String> {
    // Буфер обмена — это OLE-объект, поэтому чтение требует STA.
    let _ole = OleGuard::new();

    let mut empty_message = String::new();

    for attempt in 1..=ATTEMPTS {
        match read_image() {
            Ok(path) => {
                program::log(&format!(
                    "картинка взята из буфера обмена: {}",
                    path.display()
                ));
                return Ok(path);
            }
            Err(ClipboardError::Busy) => {
                if attempt == ATTEMPTS {
                    program::log("буфер обмена недоступен: занят другим приложением");
                    return Err("Буфер обмена занят другим приложением. Скопируйте картинку ещё \
                                раз и повторите."
                        .to_string());
                }
                thread::sleep(RETRY_DELAY);
            }
            Err(ClipboardError::Empty) => {
                empty_message = "В буфере обмена нет картинки. Скопируйте изображение (Ctrl+C) \
                                 и нажмите «Вставить» ещё раз."
                    .to_string();
            }
            Err(ClipboardError::Other(error)) => {
                program::log(&format!("ошибка чтения буфера обмена: {error}"));
                return Err(error);
            }
        }
    }

    Err(empty_message)
}

enum ClipboardError {
    /// Буфер открыт другим процессом — есть смысл повторить.
    Busy,
    /// Картинки в буфере нет.
    Empty,
    Other(String),
}

/// Открывает буфер и пробует все известные форматы.
fn read_image() -> Result<PathBuf, ClipboardError> {
    // CLIPBRD_E_CANT_OPEN — буфер открыт другим процессом.
    const CLIPBRD_E_CANT_OPEN: u32 = 0x8004_01D0;

    unsafe {
        OpenClipboard(None).map_err(|e| {
            if e.code().0 as u32 == CLIPBRD_E_CANT_OPEN {
                ClipboardError::Busy
            } else {
                ClipboardError::Other(format!("буфер обмена недоступен: {e}"))
            }
        })?;

        let result = read_from_open_clipboard();
        let _ = CloseClipboard();

        result
    }
}

fn read_from_open_clipboard() -> Result<PathBuf, ClipboardError> {
    if let Some(path) = read_file_drop() {
        return Ok(path);
    }

    if let Some(path) = read_text_path() {
        return Ok(path);
    }

    for format in [CF_DIB, CF_DIBV5, CF_BITMAP] {
        if let Some(path) = read_image_format(format.0) {
            return Ok(path);
        }
    }

    Err(ClipboardError::Empty)
}

/// Файлы и папки, скопированные в буфер обмена.
fn read_file_drop() -> Option<PathBuf> {
    let handle = get_clipboard_data(CF_HDROP.0 as u32)?;

    let data = lock(handle);
    if data.is_null() {
        return None;
    }

    let result = unsafe { first_dropped_image(data.cast::<u16>()) };
    unsafe {
        let _ = GlobalUnlock(handle);
    }
    result
}

/// Разбирает список путей из `DROPFILES` и берёт первую подходящую картинку.
unsafe fn first_dropped_image(data: *const u16) -> Option<PathBuf> {
    if data.is_null() {
        return None;
    }

    // Структура DROPFILES занимает 20 байт, дальше идёт список строк.
    let header = data.cast::<u8>();
    let count = *header.add(4).cast::<u16>() as usize;
    let offset = *header.add(8).cast::<u32>() as usize;

    let mut cursor = header.add(offset);
    for _ in 0..count {
        let text = program::from_pwstr(Some(windows::core::PWSTR(
            cursor as *mut u16,
        )));
        if text.is_empty() {
            break;
        }

        if let Some(found) = first_image_in(Path::new(&text)) {
            return Some(found);
        }

        cursor = cursor.add((text.len() + 1) * 2);
    }

    None
}

/// Путь в буфере обмена, если это текст с названием картинки.
fn read_text_path() -> Option<PathBuf> {
    let handle = get_clipboard_data(CF_UNICODETEXT)?;

    let data = lock(handle);
    if data.is_null() {
        return None;
    }

    let text = program::from_pwstr(Some(windows::core::PWSTR(data.cast::<u16>())));
    unsafe {
        let _ = GlobalUnlock(handle);
    }

    let line = text.split(['\n', '\r']).next()?.trim().trim_matches('"');
    let path = Path::new(line);

    if program::is_supported_image(path) && path.is_file() {
        return Some(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()));
    }

    None
}

/// Картинка из буфера: `CF_DIB`, `CF_DIBV5` или `CF_BITMAP`.
fn read_image_format(format: u16) -> Option<PathBuf> {
    if format == CF_BITMAP.0 {
        return read_bitmap();
    }
    read_dib(format)
}

/// `CF_DIB` / `CF_DIBV5`: в буфере уже готовое DIB, разбираем его сами.
fn read_dib(format: u16) -> Option<PathBuf> {
    let handle = get_clipboard_data(format as u32)?;

    let size = unsafe { GlobalSize(handle) };
    let data = lock(handle);
    if data.is_null() || size == 0 {
        return None;
    }

    // Указатель получен из глобальной памяти буфера, длина — из GlobalSize.
    let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) };
    let image = dib_to_image(bytes, format == CF_DIBV5.0);

    unsafe {
        let _ = GlobalUnlock(handle);
    }

    let image = image?;
    save_clipboard_image(&image)
}

/// `CF_BITMAP`: готовое GDI-изображение, переносим его через `GetDIBits`.
fn read_bitmap() -> Option<PathBuf> {
    let handle = get_clipboard_data(CF_BITMAP.0 as u32)?;
    let bitmap = HBITMAP(handle.0);

    unsafe {
        let mut info: BITMAP = std::mem::zeroed();
        if GetObjectW(
            windows::Win32::Graphics::Gdi::HGDIOBJ(bitmap.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some((&mut info as *mut BITMAP).cast()),
        ) == 0
        {
            return None;
        }

        let width = info.bmWidth;
        let height = info.bmHeight.unsigned_abs();
        if width <= 0 || height == 0 {
            return None;
        }

        let mut bits = vec![0u8; width as usize * height as usize * 4];
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Отрицательная высота переворачивает строки в исходный порядок.
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                ..Default::default()
            },
            ..Default::default()
        };

        let dc: HDC = CreateCompatibleDC(None);
        let lines = GetDIBits(
            dc,
            bitmap,
            0,
            height,
            Some(bits.as_mut_ptr().cast()),
            &mut header,
            DIB_RGB_COLORS,
        );

        let _ = DeleteDC(dc);
        let _ = DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(bitmap.0));

        if lines == 0 {
            return None;
        }

        let image = DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(
                width as u32,
                height as u32,
                convert_bgra_to_rgba(&bits),
            )?,
        );

        save_clipboard_image(&image)
    }
}

/// Разбирает `DIB` в изображение.
fn dib_to_image(bytes: &[u8], is_v5: bool) -> Option<DynamicImage> {
    if bytes.len() < std::mem::size_of::<BITMAPINFOHEADER>() {
        return None;
    }

    let header: BITMAPINFOHEADER =
        unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<BITMAPINFOHEADER>()) };

    if header.biWidth <= 0 || header.biPlanes != 1 {
        return None;
    }

    let width = header.biWidth as u32;
    let height = header.biHeight.unsigned_abs();
    let bits = header.biBitCount;
    if width == 0 || height == 0 || !matches!(bits, 1 | 4 | 8 | 16 | 24 | 32) {
        return None;
    }

    // Длина строки в DIB всегда кратна 4 байтам.
    let stride = ((width * bits as u32 + 31) / 32) * 4;

    // Палитра идёт сразу после заголовка и есть только у 1/4/8-битных картинок.
    let palette_start = std::mem::size_of::<BITMAPINFOHEADER>();
    let palette = if bits <= 8 {
        let start = header.biSize.max(palette_start as u32) as usize;
        bytes.get(palette_start..start.min(bytes.len()))?
    } else {
        &[][..]
    };
    let pixel_start = if bits <= 8 {
        (header.biSize.max(palette_start as u32) as usize).min(bytes.len())
    } else {
        header.biSize as usize
    };

    // V5 и положительная высота: строки идут сверху вниз.
    let top_down = header.biHeight < 0 || is_v5;

    let needed = pixel_start + (stride * height) as usize;
    let pixels = bytes.get(pixel_start..needed)?;

    let rgba = decode_bits(pixels, width, height, stride, bits, palette, !top_down)?;
    Some(DynamicImage::ImageRgba8(rgba))
}

/// Раскладывает пиксели DIB в RGBA.
fn decode_bits(
    pixels: &[u8],
    width: u32,
    height: u32,
    stride: u32,
    bits: u16,
    palette: &[u8],
    flip_vertical: bool,
) -> Option<image::RgbaImage> {
    let mut rgba = image::RgbaImage::new(width, height);

    for y in 0..height {
        let source_row = if flip_vertical { height - 1 - y } else { y };
        let row_start = (source_row * stride) as usize;
        let row = pixels.get(row_start..)?;

        for x in 0..width as usize {
            let target = ((y * width + x as u32) * 4) as usize;
            let color = match bits {
                32 => {
                    let offset = x * 4;
                    // Каналы в DIB идут как B, G, R, A.
                    [
                        *row.get(offset + 2)?,
                        *row.get(offset + 1)?,
                        *row.get(offset)?,
                        *row.get(offset + 3).unwrap_or(&255),
                    ]
                }
                24 => {
                    let offset = x * 3;
                    [
                        *row.get(offset + 2)?,
                        *row.get(offset + 1)?,
                        *row.get(offset)?,
                        255,
                    ]
                }
                16 => {
                    let offset = x * 2;
                    let value = u16::from_le_bytes([*row.get(offset)?, *row.get(offset + 1)?]);
                    unpack_16(value)
                }
                8 | 4 | 1 => {
                    let index = match bits {
                        8 => *row.get(x)? as usize,
                        4 => {
                            let byte = *row.get(x / 2)?;
                            let shift = if x % 2 == 0 { 4 } else { 0 };
                            ((byte >> shift) & 0x0F) as usize
                        }
                        _ => {
                            let byte = *row.get(x / 8)?;
                            ((byte >> (7 - x % 8)) & 0x01) as usize
                        }
                    };

                    let offset = index * 4;
                    [
                        *palette.get(offset + 2)?,
                        *palette.get(offset + 1)?,
                        *palette.get(offset)?,
                        255,
                    ]
                }
                _ => return None,
            };

            rgba.as_mut()[target..target + 4].copy_from_slice(&color);
        }
    }

    Some(rgba)
}

/// Раскладывает 16 бит в RGB по схеме 5-5-5 или 5-6-5.
fn unpack_16(value: u16) -> [u8; 4] {
    let component = |shift: u32| ((value >> shift) & 0x1F) as u8 * 255 / 31;

    // Бит 15 вместе с битом 14 — признак ключевого цвета.
    if value & 0x7C00 == 0x7C00 {
        return [0, 0, 0, 255];
    }

    [component(10), component(5), component(0), 255]
}

/// BGRA → RGBA для картинок, полученных через `GetDIBits`.
fn convert_bgra_to_rgba(bits: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bits.len());
    for pixel in bits.chunks_exact(4) {
        out.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    out
}

/// Сохраняет картинку из буфера в папку данных как `wallpaper.png`.
fn save_clipboard_image(image: &DynamicImage) -> Option<PathBuf> {
    let path = program::data_dir().join(program::CLIPBOARD_FILE);
    images::save_png(image, &path).ok()?;
    Some(path)
}

/// Первая подходящая картинка: сам файл или первая в папке по алфавиту.
fn first_image_in(path: &Path) -> Option<PathBuf> {
    if path.is_file() && program::is_supported_image(path) {
        return Some(path.to_path_buf());
    }

    if path.is_dir() {
        let mut found: Vec<PathBuf> = std::fs::read_dir(path)
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .filter(|p| program::is_supported_image(p))
            .collect();

        found.sort_by_key(|p| p.to_string_lossy().to_lowercase());
        return found.into_iter().next();
    }

    None
}

fn get_clipboard_data(format: u32) -> Option<HGLOBAL> {
    // Формата может не быть в буфере — тогда это не ошибка, а пустой результат.
    unsafe { GetClipboardData(format).ok().map(|h| HGLOBAL(h.0)) }
}

/// Блокирует глобальную память, возвращая нулевой указатель при неудаче.
fn lock(handle: HGLOBAL) -> *mut std::ffi::c_void {
    unsafe { GlobalLock(handle) }
}

/// Инициализирует OLE ровно на время чтения буфера.
struct OleGuard {
    active: bool,
}

impl OleGuard {
    fn new() -> OleGuard {
        // Успех означает, что счётчик инициализации надо будет снять.
        OleGuard { active: unsafe { OleInitialize(None) }.is_ok() }
    }
}

impl Drop for OleGuard {
    fn drop(&mut self) {
        if self.active {
            unsafe {
                OleUninitialize();
            }
        }
    }
}