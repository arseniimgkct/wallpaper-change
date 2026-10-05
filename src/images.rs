//! Работа с картинками: загрузка, сохранение, превью и масштабирование «по
//! размеру окна» так же, как это делал GDI+ в оригинале.

use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use image::{DynamicImage, RgbaImage};

use crate::program;

/// Изображение в виде, пригодном для отрисовки через GDI.
pub struct Wallpaper {
    /// Масштабированное изображение в формате BGRA (как ждёт `StretchDIBits`).
    pub bgra: Vec<u8>,
    pub width: i32,
    pub height: i32,
}

impl Wallpaper {
    /// Готовит картинку под размер окна: вписывает с обрезкой (cover) и
    /// растягивает с бикубической фильтрацией — как `HighQualityBicubic` в
    /// GDI+; ближайший фильтр в `image` — CatmullRom.
    pub fn scaled_to(source: &DynamicImage, width: i32, height: i32) -> Option<Wallpaper> {
        if width <= 0 || height <= 0 {
            return None;
        }

        let (src_w, src_h) = (source.width().max(1), source.height().max(1));
        let scale = (width as f64 / src_w as f64).max(height as f64 / src_h as f64);

        let target_w = ((src_w as f64 * scale).round() as u32).max(1);
        let target_h = ((src_h as f64 * scale).round() as u32).max(1);

        // Обрезаем лишнее по центру: так «cover» и ведёт себя GDI+.
        let scaled = source.resize_exact(target_w, target_h, FilterType::CatmullRom);

        // Если картинка уже точно по размеру окна, обрезки не будет.
        let scaled = if scaled.width() == width as u32 && scaled.height() == height as u32 {
            scaled
        } else {
            scaled.crop_imm(
                scaled.width().saturating_sub(width as u32) / 2,
                scaled.height().saturating_sub(height as u32) / 2,
                width as u32,
                height as u32,
            )
        };

        Some(Wallpaper { bgra: to_bgra(&scaled.to_rgba8()), width, height })
    }
}

/// Переводит RGBA в BGRA — порядок каналов, который ожидает GDI.
fn to_bgra(rgba: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.as_raw().len());
    for pixel in rgba.as_raw().chunks_exact(4) {
        out.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    out
}

/// Загружает картинку с диска.
pub fn load(path: &Path) -> Result<DynamicImage, String> {
    image::open(path).map_err(|e| e.to_string())
}

/// Читает картинку, не заворачивая ошибку в строку с путём.
pub fn probe(path: &Path) -> Option<DynamicImage> {
    load(path).ok()
}

/// Сохраняет картинку в PNG.
pub fn save_png(image: &DynamicImage, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    image.save_with_format(path, image::ImageFormat::Png).map_err(|e| e.to_string())
}

/// Создаёт PNG одного цвета размером с экран.
///
/// Windows не умеет ставить сплошной цвет как обои, поэтому мы генерируем
/// картинку — ровно так же поступал оригинал.
pub fn solid_color(color: [u8; 3]) -> Result<PathBuf, String> {
    let (width, height) = program::screen_size_for_wallpaper();

    let mut buffer = Vec::with_capacity((width * height * 3) as usize);
    for _ in 0..(width * height) {
        buffer.extend_from_slice(&color);
    }

    let raw = image::RgbImage::from_raw(width, height, buffer)
        .ok_or_else(|| "не удалось создать изображение цвета".to_string())?;
    let image = DynamicImage::ImageRgb8(raw);

    let path = program::data_dir().join(program::SOLID_COLOR_FILE);
    save_png(&image, &path)?;
    Ok(path)
}

/// `#RRGGBB` для сообщений в интерфейсе.
pub fn hex(color: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2])
}

/// Готовит устойчивую копию картинки в папке данных.
///
/// Копия нужна, потому что исходный файл пользователь может переместить или
/// изменить, а оверлей и автозапуск ссылаются именно на копию.
pub fn stable_copy(source: &Path) -> Result<DynamicImage, String> {
    let dir = program::data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let ext = source
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| "png".to_string());
    let target = dir.join(format!("wallpaper.{ext}"));

    if needs_copy(source, &target) {
        std::fs::copy(source, &target).map_err(|e| e.to_string())?;
    }

    load(&target)
}

/// Копировать заново, если цели нет, она меньше источника или старее его.
fn needs_copy(source: &Path, target: &Path) -> bool {
    let Ok(source_meta) = std::fs::metadata(source) else {
        return true;
    };
    let Ok(target_meta) = std::fs::metadata(target) else {
        return true;
    };

    let modified = |meta: &std::fs::Metadata| {
        meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
    };

    if source_meta.len() != target_meta.len() {
        return true;
    }

    match (modified(&source_meta), modified(&target_meta)) {
        (Some(source_time), Some(target_time)) => target_time < source_time,
        _ => true,
    }
}

/// Удаляет сохранённые копии картинки. `true`, если что-то удалено.
pub fn delete_saved_images() -> bool {
    let dir = program::data_dir();
    if !dir.is_dir() {
        return false;
    }

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return false;
    };

    let mut removed = false;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name.starts_with("wallpaper.") {
            if std::fs::remove_file(entry.path()).is_ok() {
                removed = true;
            }
        }
    }
    removed
}