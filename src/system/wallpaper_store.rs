use std::fs::{create_dir_all, remove_file};
use std::path::{Path, PathBuf};
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use crate::system::paths::{get_app_dir, get_default_saved_wallpaper_path};
use crate::logger::log_message;

pub const SUPPORTED_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "bmp", "gif", "webp", "tif", "tiff"];

pub fn is_supported_image(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext_lower = ext.to_lowercase();
        SUPPORTED_EXTENSIONS.contains(&ext_lower.as_str())
    } else {
        false
    }
}

pub fn clean_saved_wallpapers() {
    let app_dir = get_app_dir();
    for ext in SUPPORTED_EXTENSIONS {
        let p = app_dir.join(format!("wallpaper.{}", ext));
        if p.exists() {
            let _ = remove_file(&p);
        }
    }
}

pub fn save_image_copy(source_path: &Path) -> Result<PathBuf, String> {
    if !source_path.is_file() {
        return Err(format!("Source path is not a file: {:?}", source_path));
    }

    let app_dir = get_app_dir();
    create_dir_all(&app_dir).map_err(|e| format!("Failed to create AppData directory: {}", e))?;

    clean_saved_wallpapers();

    let ext = source_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_else(|| "png".to_string());

    let target_path = get_default_saved_wallpaper_path(&ext);
    std::fs::copy(source_path, &target_path)
        .map_err(|e| format!("Failed to copy image to {:?}: {}", target_path, e))?;

    log_message(&format!("Сохранена копия обоев: {:?}", target_path));
    Ok(target_path)
}

pub fn save_solid_color(r: u8, g: u8, b: u8, screen_w: u32, screen_h: u32) -> Result<PathBuf, String> {
    let app_dir = get_app_dir();
    create_dir_all(&app_dir).map_err(|e| format!("Failed to create AppData directory: {}", e))?;

    clean_saved_wallpapers();

    let w = screen_w.max(1920);
    let h = screen_h.max(1080);

    let mut img = RgbImage::new(w, h);
    let pixel = Rgb([r, g, b]);
    for p in img.pixels_mut() {
        *p = pixel;
    }

    let target_path = get_default_saved_wallpaper_path("png");
    DynamicImage::ImageRgb8(img)
        .save_with_format(&target_path, ImageFormat::Png)
        .map_err(|e| format!("Failed to save solid color image: {}", e))?;

    log_message(&format!(
        "Сгенерирован сплошной цвет RGB({},{},{}) {}x{} -> {:?}",
        r, g, b, w, h, target_path
    ));
    Ok(target_path)
}

pub fn save_dynamic_image(img: &DynamicImage) -> Result<PathBuf, String> {
    let app_dir = get_app_dir();
    create_dir_all(&app_dir).map_err(|e| format!("Failed to create AppData directory: {}", e))?;

    clean_saved_wallpapers();

    let target_path = get_default_saved_wallpaper_path("png");
    img.save_with_format(&target_path, ImageFormat::Png)
        .map_err(|e| format!("Failed to save dynamic image: {}", e))?;

    log_message(&format!("Изображение сохранено в {:?}", target_path));
    Ok(target_path)
}
