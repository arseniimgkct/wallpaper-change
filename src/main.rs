#![windows_subsystem = "windows"]

mod ipc;
mod logger;
mod overlay;
mod system;
mod tray;
mod ui;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::ipc::{send_ipc_command, start_ipc_server, IpcCommand, SingleInstanceLock};
use crate::logger::log_message;
use crate::overlay::engine::OverlayManager;
use crate::system::revert::revert_all;
use crate::system::wallpaper_store::save_image_copy;
use crate::tray::TrayManager;
use crate::ui::app::DesktopOverlayApp;

enum CliMode {
    Gui { initial_path: Option<PathBuf> },
    HeadlessApply { path: PathBuf },
    Revert { restore_light: bool },
}

fn parse_cli_args() -> CliMode {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    let mut is_revert = false;
    let mut restore_light = false;
    let mut apply_path: Option<PathBuf> = None;
    let mut set_path: Option<PathBuf> = None;

    while i < args.len() {
        match args[i].as_str() {
            "--revert" | "-revert" | "/revert" => {
                is_revert = true;
            }
            "--light" | "-light" | "/light" => {
                restore_light = true;
            }
            "--apply" | "-apply" | "/apply" => {
                if i + 1 < args.len() {
                    apply_path = Some(PathBuf::from(&args[i + 1]));
                    i += 1;
                }
            }
            "--set" | "-set" | "/set" => {
                if i + 1 < args.len() {
                    set_path = Some(PathBuf::from(&args[i + 1]));
                    i += 1;
                }
            }
            other => {
                let p = PathBuf::from(other);
                if p.is_file() {
                    set_path = Some(p);
                }
            }
        }
        i += 1;
    }

    if is_revert {
        CliMode::Revert { restore_light }
    } else if let Some(path) = apply_path {
        CliMode::HeadlessApply { path }
    } else {
        CliMode::Gui { initial_path: set_path }
    }
}

fn load_app_icon_data() -> Option<egui::IconData> {
    let ico_bytes = include_bytes!("../app.ico");
    if let Ok(dyn_img) = image::load_from_memory(ico_bytes) {
        let rgba = dyn_img.to_rgba8();
        let (width, height) = (rgba.width(), rgba.height());
        Some(egui::IconData {
            rgba: rgba.into_raw(),
            width,
            height,
        })
    } else {
        None
    }
}

fn run_headless_tray_loop(
    overlay_mgr: Arc<OverlayManager>,
    path: PathBuf,
    ipc_rx: std::sync::mpsc::Receiver<IpcCommand>,
) {
    if let Ok(saved) = save_image_copy(&path) {
        overlay_mgr.apply_image(saved);
    }

    let tray_mgr = match TrayManager::new() {
        Ok(t) => Some(t),
        Err(e) => {
            log_message(&format!("Не удалось создать трей в headless режиме: {}", e));
            None
        }
    };

    log_message("Headless режим активен. Ожидание событий трея/IPC...");

    let is_running = Arc::new(AtomicBool::new(true));

    while is_running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(100));

        // IPC handling
        while let Ok(cmd) = ipc_rx.try_recv() {
            match cmd {
                IpcCommand::Show => {
                    log_message("Получена команда SHOW в headless режиме");
                }
                IpcCommand::Revert => {
                    let _ = revert_all(&overlay_mgr, false);
                    is_running.store(false, Ordering::SeqCst);
                }
                IpcCommand::Set(new_path) => {
                    if let Ok(saved) = save_image_copy(&new_path) {
                        overlay_mgr.apply_image(saved);
                    }
                }
            }
        }

        // Tray menu handling
        if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if let Some(ref tray) = tray_mgr {
                if event.id == tray.reattach_item_id {
                    overlay_mgr.force_reattach();
                } else if event.id == tray.revert_item_id {
                    let _ = revert_all(&overlay_mgr, false);
                } else if event.id == tray.exit_item_id {
                    overlay_mgr.remove_overlay();
                    is_running.store(false, Ordering::SeqCst);
                }
            }
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    log_message("Запуск DesktopOverlay...");

    let cli_mode = parse_cli_args();
    let lock = SingleInstanceLock::try_acquire();

    if !lock.is_first_instance() {
        log_message("Обнаружен уже запущенный экземпляр DesktopOverlay, передача команды через IPC...");
        match cli_mode {
            CliMode::Revert { .. } => {
                let _ = send_ipc_command(&IpcCommand::Revert);
            }
            CliMode::HeadlessApply { path } | CliMode::Gui { initial_path: Some(path) } => {
                let _ = send_ipc_command(&IpcCommand::Set(path));
            }
            CliMode::Gui { initial_path: None } => {
                let _ = send_ipc_command(&IpcCommand::Show);
            }
        }
        return Ok(());
    }

    let overlay_mgr = OverlayManager::new();
    let (ipc_tx, ipc_rx) = channel::<IpcCommand>();
    let is_running = Arc::new(AtomicBool::new(true));

    start_ipc_server(ipc_tx, Arc::clone(&is_running));

    match cli_mode {
        CliMode::Revert { restore_light } => {
            log_message("Запуск в режиме отката изменений (--revert)...");
            let report = revert_all(&overlay_mgr, restore_light);
            println!("Откат выполнен: {}", report);
            return Ok(());
        }
        CliMode::HeadlessApply { path } => {
            log_message(&format!("Запуск в headless режиме с обоями: {:?}", path));
            run_headless_tray_loop(overlay_mgr, path, ipc_rx);
            return Ok(());
        }
        CliMode::Gui { initial_path } => {
            log_message("Запуск в стандартном GUI режиме...");

            let tray_mgr = match TrayManager::new() {
                Ok(t) => Some(t),
                Err(e) => {
                    log_message(&format!("Предупреждение: трей не инициализирован: {}", e));
                    None
                }
            };

            let mut viewport = egui::ViewportBuilder::default()
                .with_title("DesktopOverlay")
                .with_inner_size([580.0, 680.0])
                .with_min_inner_size([480.0, 520.0])
                .with_drag_and_drop(true);

            if let Some(icon) = load_app_icon_data() {
                viewport = viewport.with_icon(icon);
            }

            let native_options = eframe::NativeOptions {
                viewport,
                ..Default::default()
            };

            let overlay_mgr_clone = Arc::clone(&overlay_mgr);

            eframe::run_native(
                "DesktopOverlay",
                native_options,
                Box::new(move |cc| {
                    // Dark theme by default
                    cc.egui_ctx.set_visuals(egui::Visuals::dark());

                    Ok(Box::new(DesktopOverlayApp::new(
                        cc,
                        overlay_mgr_clone,
                        tray_mgr,
                        ipc_rx,
                        initial_path,
                    )))
                }),
            )
        }
    }
}
