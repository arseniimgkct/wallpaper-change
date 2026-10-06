use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use egui::{Color32, ColorImage, Context, TextureHandle, TextureOptions, Vec2};
use tray_icon::menu::MenuEvent;
use tray_icon::TrayIconEvent;

use crate::ipc::IpcCommand;
use crate::overlay::engine::OverlayManager;
use crate::system::autostart::{is_autostart_enabled, set_autostart};
use crate::system::browser::{
    find_chrome_executable, find_firefox_executable, get_current_default_browser,
    make_chrome_default, make_firefox_default, unpin_edge_from_taskbar,
};
use crate::system::clipboard::read_clipboard_with_retry;
use crate::system::color_picker::choose_color;
use crate::system::paths::get_saved_wallpaper_path;
use crate::system::revert::revert_all;
use crate::system::taskbar::{is_taskbar_small_icons, is_windows_11, set_taskbar_small_icons};
use crate::system::theme::{is_light_theme, set_light_theme};
use crate::system::wallpaper_store::{
    is_supported_image, save_image_copy, save_solid_color, SUPPORTED_EXTENSIONS,
};
use crate::tray::TrayManager;

pub struct DesktopOverlayApp {
    overlay_mgr: Arc<OverlayManager>,
    tray_mgr: Option<TrayManager>,
    ipc_rx: Receiver<IpcCommand>,
    selected_image_path: Option<PathBuf>,
    preview_texture: Option<TextureHandle>,
    status_message: String,
    autostart_active: bool,
    show_revert_modal: bool,
    revert_audit_result: Option<String>,
}

impl DesktopOverlayApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        overlay_mgr: Arc<OverlayManager>,
        tray_mgr: Option<TrayManager>,
        ipc_rx: Receiver<IpcCommand>,
        initial_image: Option<PathBuf>,
    ) -> Self {
        let saved_path = initial_image.or_else(get_saved_wallpaper_path);
        let autostart_active = is_autostart_enabled();

        let mut app = Self {
            overlay_mgr,
            tray_mgr,
            ipc_rx,
            selected_image_path: saved_path,
            preview_texture: None,
            status_message: "Готово к работе".to_string(),
            autostart_active,
            show_revert_modal: false,
            revert_audit_result: None,
        };

        if let Some(ref path) = app.selected_image_path.clone() {
            app.apply_image_file(path.clone());
        }

        app
    }

    fn apply_image_file(&mut self, path: PathBuf) {
        if path.is_file() {
            match save_image_copy(&path) {
                Ok(saved_path) => {
                    self.selected_image_path = Some(saved_path.clone());
                    self.overlay_mgr.apply_image(saved_path);
                    self.status_message = format!("Обои применены: {:?}", path.file_name().unwrap_or_default());
                    self.preview_texture = None;
                }
                Err(e) => {
                    self.status_message = format!("Ошибка применения: {}", e);
                }
            }
        }
    }

    fn apply_solid_color_preset(&mut self, r: u8, g: u8, b: u8, label: &str) {
        let screen_w = unsafe { windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN) } as u32;
        let screen_h = unsafe { windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN) } as u32;

        match save_solid_color(r, g, b, screen_w, screen_h) {
            Ok(saved_path) => {
                self.selected_image_path = Some(saved_path.clone());
                self.overlay_mgr.apply_image(saved_path);
                self.status_message = format!("Применен сплошной цвет: {}", label);
                self.preview_texture = None;
            }
            Err(e) => {
                self.status_message = format!("Ошибка создания цвета: {}", e);
            }
        }
    }

    fn handle_clipboard_paste(&mut self) {
        match read_clipboard_with_retry() {
            Ok(saved_path) => {
                self.selected_image_path = Some(saved_path.clone());
                self.overlay_mgr.apply_image(saved_path);
                self.status_message = "Изображение из буфера обмена успешно применено!".to_string();
                self.preview_texture = None;
            }
            Err(e) => {
                self.status_message = format!("Буфер обмена: {}", e);
            }
        }
    }

    fn handle_file_drop(&mut self, dropped_files: &[egui::DroppedFile]) {
        for file in dropped_files {
            if let Some(ref path) = file.path {
                if path.is_file() && is_supported_image(path) {
                    self.apply_image_file(path.clone());
                    break;
                } else if path.is_dir() {
                    if let Ok(entries) = std::fs::read_dir(path) {
                        let mut candidates: Vec<PathBuf> = entries
                            .filter_map(|e| e.ok())
                            .map(|e| e.path())
                            .filter(|p| p.is_file() && is_supported_image(p))
                            .collect();
                        candidates.sort_by(|a, b| {
                            a.file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_lowercase()
                                .cmp(&b.file_name().unwrap_or_default().to_string_lossy().to_lowercase())
                        });
                        if let Some(first) = candidates.into_iter().next() {
                            self.apply_image_file(first);
                            break;
                        }
                    }
                }
            }
        }
    }

    fn load_preview(&mut self, ctx: &Context) {
        if self.preview_texture.is_some() {
            return;
        }

        if let Some(ref path) = self.selected_image_path {
            if let Ok(reader) = image::ImageReader::open(path) {
                if let Ok(dyn_img) = reader.decode() {
                    let thumb = dyn_img.thumbnail(320, 180);
                    let rgba = thumb.to_rgba8();
                    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
                    let color_img = ColorImage::from_rgba_unmultiplied([w, h], &rgba.into_raw());
                    self.preview_texture = Some(ctx.load_texture("preview", color_img, TextureOptions::LINEAR));
                }
            }
        }
    }

    fn check_ipc_and_tray(&mut self, ctx: &Context) {
        // Handle IPC commands
        while let Ok(cmd) = self.ipc_rx.try_recv() {
            match cmd {
                IpcCommand::Show => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                IpcCommand::Revert => {
                    let report = revert_all(&self.overlay_mgr, false);
                    self.revert_audit_result = Some(report);
                    self.selected_image_path = None;
                    self.preview_texture = None;
                }
                IpcCommand::Set(path) => {
                    self.apply_image_file(path);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            }
        }

        // Handle Tray menu events
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(ref tray) = self.tray_mgr {
                if event.id == tray.show_item_id {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                } else if event.id == tray.reattach_item_id {
                    self.overlay_mgr.force_reattach();
                    self.status_message = "Оверлей принудительно переприкреплен".to_string();
                } else if event.id == tray.revert_item_id {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.show_revert_modal = true;
                } else if event.id == tray.exit_item_id {
                    self.overlay_mgr.remove_overlay();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        // Handle Tray double click / click
        if let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::DoubleClick { .. } = event {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
    }
}

impl eframe::App for DesktopOverlayApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.check_ipc_and_tray(ctx);
        self.load_preview(ctx);

        // Handle Drag and Drop
        if !ctx.input(|i| i.raw.dropped_files.is_empty()) {
            let files = ctx.input(|i| i.raw.dropped_files.clone());
            self.handle_file_drop(&files);
        }

        // Handle Ctrl+V shortcut
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::V)) {
            self.handle_clipboard_paste();
        }

        // Intercept window close (X) to hide to tray
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // Confirmation Modal for Revert
        if self.show_revert_modal {
            egui::Window::new("Подтверждение отката")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Вы действительно хотите полностью снять оверлей и откатить все изменения?");
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button(" Да, откатить всё ").clicked() {
                            let report = revert_all(&self.overlay_mgr, false);
                            self.revert_audit_result = Some(report);
                            self.selected_image_path = None;
                            self.preview_texture = None;
                            self.autostart_active = false;
                            self.status_message = "Все изменения успешно откатаны".to_string();
                            self.show_revert_modal = false;
                        }
                        if ui.button(" Отмена ").clicked() {
                            self.show_revert_modal = false;
                        }
                    });
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("🖼 DesktopOverlay — Обои под иконками рабочего стола");
                ui.label(egui::RichText::new("Установка картинки или цвета без изменения системных обоев").weak());
                ui.separator();

                // Section 1: Image Selection
                ui.group(|ui| {
                    ui.heading("1. Выбор изображения");
                    ui.horizontal(|ui| {
                        if ui.button("📁 Выбрать файл...").clicked() {
                            let dialog = rfd::FileDialog::new()
                                .add_filter("Изображения", SUPPORTED_EXTENSIONS)
                                .add_filter("Все файлы", &["*"]);

                            if let Some(path) = dialog.pick_file() {
                                self.apply_image_file(path);
                            }
                        }

                        if ui.button("📋 Вставить из буфера (Ctrl+V)").clicked() {
                            self.handle_clipboard_paste();
                        }
                    });

                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Или перетащите файл/папку с картинкой прямо в это окно (Drag & Drop)").italics());

                    if let Some(ref texture) = self.preview_texture {
                        ui.add_space(8.0);
                        ui.label("Предпросмотр:");
                        ui.image((texture.id(), Vec2::new(260.0, 146.0)));
                    }

                    if let Some(ref p) = self.selected_image_path {
                        ui.label(format!("Текущий файл: {}", p.display()));
                    }
                });

                ui.add_space(8.0);

                // Section 2: Solid Colors
                ui.group(|ui| {
                    ui.heading("2. Сплошные цвета (Solid Colors)");
                    ui.horizontal_wrapped(|ui| {
                        let presets = [
                            (0x00, 0x00, 0x00, "Чёрный (#000000)"),
                            (0x12, 0x12, 0x12, "Тёмный графит (#121212)"),
                            (0x1C, 0x21, 0x28, "Тёмно-синий (#1C2128)"),
                            (0x25, 0x2A, 0x34, "Сланцевый (#252A34)"),
                            (0x32, 0x32, 0x32, "Серый (#323232)"),
                            (0xF5, 0xF5, 0xF5, "Белый (#F5F5F5)"),
                        ];

                        for (r, g, b, name) in presets {
                            let btn = egui::Button::new(name)
                                .fill(Color32::from_rgb(r, g, b))
                                .stroke(egui::Stroke::new(1.0_f32, Color32::GRAY));
                            if ui.add(btn).clicked() {
                                self.apply_solid_color_preset(r, g, b, name);
                            }
                        }
                    });

                    ui.add_space(4.0);
                    if ui.button("🎨 Выбрать свой цвет (Палитра Windows)...").clicked() {
                        if let Some((r, g, b)) = choose_color(0x12, 0x12, 0x12) {
                            let label = format!("RGB({},{},{})", r, g, b);
                            self.apply_solid_color_preset(r, g, b, &label);
                        }
                    }
                });

                ui.add_space(8.0);

                // Section 3: System Tweaks
                ui.group(|ui| {
                    ui.heading("3. Системные настройки и твики");

                    // Theme toggle
                    let light = is_light_theme();
                    let theme_label = if light {
                        "🌙 Сменить тему Windows на тёмную"
                    } else {
                        "☀ Сменить тему Windows на светлую"
                    };
                    if ui.button(theme_label).clicked() {
                        match set_light_theme(!light) {
                            Ok(changed) => {
                                if changed {
                                    self.status_message = "Тема Windows изменена".to_string();
                                }
                            }
                            Err(e) => self.status_message = format!("Ошибка смены темы: {}", e),
                        }
                    }

                    ui.add_space(4.0);

                    // Taskbar icon size
                    let win11 = is_windows_11();
                    let small_taskbar = is_taskbar_small_icons();
                    let taskbar_btn_label = if small_taskbar {
                        "Установить стандартный размер значков панели задач (40px)"
                    } else {
                        "Установить маленькие значки панели задач (30px)"
                    };

                    ui.add_enabled_ui(!win11, |ui| {
                        if ui.button(taskbar_btn_label).clicked() {
                            match set_taskbar_small_icons(!small_taskbar) {
                                Ok(changed) => {
                                    if changed {
                                        self.status_message = "Размер значков панели задач изменен".to_string();
                                    }
                                }
                                Err(e) => self.status_message = format!("Ошибка панели задач: {}", e),
                            }
                        }
                    });
                    if win11 {
                        ui.label(egui::RichText::new("ℹ В Windows 11 настройка удалена Microsoft").weak());
                    }

                    ui.add_space(6.0);
                    ui.separator();

                    // Browser Management
                    let default_browser = get_current_default_browser();
                    ui.label(format!("Текущий браузер по умолчанию: {}", default_browser.name));

                    ui.horizontal(|ui| {
                        if ui.button("Открепить Edge от панели задач").clicked() {
                            let _ = unpin_edge_from_taskbar();
                            self.status_message = "Команда открепления Edge выполнена".to_string();
                        }

                        if find_firefox_executable().is_some() {
                            if ui.button("Сделать Firefox по умолчанию").clicked() {
                                let _ = make_firefox_default();
                            }
                        }

                        if find_chrome_executable().is_some() {
                            if ui.button("Сделать Chrome по умолчанию").clicked() {
                                let _ = make_chrome_default();
                            }
                        }
                    });
                });

                ui.add_space(8.0);

                // Section 4: Autostart & Actions
                ui.group(|ui| {
                    ui.heading("4. Автозапуск и управление");

                    let mut autostart = self.autostart_active;
                    if ui.checkbox(&mut autostart, "Запускать DesktopOverlay при входе в Windows").changed() {
                        match set_autostart(autostart) {
                            Ok(()) => {
                                self.autostart_active = autostart;
                                self.status_message = if autostart {
                                    "Автозапуск активирован".to_string()
                                } else {
                                    "Автозапуск отключен".to_string()
                                };
                            }
                            Err(e) => {
                                self.status_message = format!("Ошибка автозапуска: {}", e);
                            }
                        }
                    }

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("🔄 Переприкрепить обои").clicked() {
                            self.overlay_mgr.force_reattach();
                            self.status_message = "Оверлей переприкреплен к рабочему столу".to_string();
                        }

                        let revert_btn = egui::Button::new("❌ Откатить всё (Revert)")
                            .fill(Color32::from_rgb(180, 50, 50));
                        if ui.add(revert_btn).clicked() {
                            self.show_revert_modal = true;
                        }
                    });
                });

                if let Some(ref audit) = self.revert_audit_result {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(format!("Отчёт об откате: {}", audit)).color(Color32::LIGHT_GREEN));
                }

                ui.add_space(10.0);
                ui.separator();
                ui.label(egui::RichText::new(format!("Статус: {}", self.status_message)).strong());
            });
        });

        // Request repaint to process continuous events
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
}
