use std::path::PathBuf;

pub fn get_app_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata).join("DesktopOverlay")
    } else {
        PathBuf::from("DesktopOverlay")
    }
}

pub fn get_saved_wallpaper_path() -> Option<PathBuf> {
    let dir = get_app_dir();
    if !dir.exists() {
        return None;
    }
    let supported_extensions = ["png", "jpg", "jpeg", "bmp", "gif", "webp", "tif", "tiff"];
    for ext in &supported_extensions {
        let candidate = dir.join(format!("wallpaper.{}", ext));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn get_default_saved_wallpaper_path(ext: &str) -> PathBuf {
    get_app_dir().join(format!("wallpaper.{}", ext))
}

pub fn get_system_transcoded_wallpaper_path() -> Option<PathBuf> {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let transcoded = PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Themes")
            .join("TranscodedWallpaper");
        if transcoded.is_file() {
            return Some(transcoded);
        }
    }
    None
}

pub fn get_user_pinned_taskbar_dir() -> Option<PathBuf> {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let pinned = PathBuf::from(appdata)
            .join("Microsoft")
            .join("Internet Explorer")
            .join("Quick Launch")
            .join("User Pinned")
            .join("TaskBar");
        if pinned.exists() {
            return Some(pinned);
        }
    }
    None
}
