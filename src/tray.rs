use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use crate::logger::log_message;

pub struct TrayManager {
    _tray_icon: TrayIcon,
    pub show_item_id: tray_icon::menu::MenuId,
    pub reattach_item_id: tray_icon::menu::MenuId,
    pub revert_item_id: tray_icon::menu::MenuId,
    pub exit_item_id: tray_icon::menu::MenuId,
}

impl TrayManager {
    pub fn new() -> Result<Self, String> {
        let tray_menu = Menu::new();

        let show_item = MenuItem::new("Показать окно утилиты", true, None);
        let reattach_item = MenuItem::new("Переприкрепить обои", true, None);
        let revert_item = MenuItem::new("Откатить обои", true, None);
        let separator = PredefinedMenuItem::separator();
        let exit_item = MenuItem::new("Выход", true, None);

        let show_id = show_item.id().clone();
        let reattach_id = reattach_item.id().clone();
        let revert_id = revert_item.id().clone();
        let exit_id = exit_item.id().clone();

        tray_menu.append_items(&[
            &show_item,
            &reattach_item,
            &revert_item,
            &separator,
            &exit_item,
        ]).map_err(|e| format!("Failed to build tray menu: {}", e))?;

        // Load icon from embedded app.ico
        let ico_bytes = include_bytes!("../app.ico");
        let icon = match image::load_from_memory(ico_bytes) {
            Ok(dyn_img) => {
                let rgba = dyn_img.to_rgba8();
                let (w, h) = (rgba.width(), rgba.height());
                Icon::from_rgba(rgba.into_raw(), w, h).map_err(|e| format!("Icon from rgba failed: {}", e))?
            }
            Err(e) => {
                log_message(&format!("Не удалось загрузить app.ico: {}", e));
                let fallback = vec![100u8; 16 * 16 * 4];
                Icon::from_rgba(fallback, 16, 16).map_err(|e| format!("Fallback icon failed: {}", e))?
            }
        };

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip("DesktopOverlay")
            .with_icon(icon)
            .build()
            .map_err(|e| format!("Failed to build tray icon: {}", e))?;

        Ok(Self {
            _tray_icon: tray_icon,
            show_item_id: show_id,
            reattach_item_id: reattach_id,
            revert_item_id: revert_id,
            exit_item_id: exit_id,
        })
    }
}
