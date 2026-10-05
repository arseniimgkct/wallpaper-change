//! Обои рабочего стола — порт на Rust.
//!
//! Приложение ставит картинку или сплошной цвет на рабочий стол поверх
//! системных обоев: оверлей живёт внутри окна рабочего стола Windows,
//! поэтому картинка оказывается под значками и панелью задач.

mod app;
mod browser;
mod clipboard;
mod engine;
mod images;
mod native;
mod overlay;
mod program;
mod reverter;
mod shell;
mod taskbar;
mod taskbar_pins;
mod theme;
mod ui;

fn main() {
    program::enable_per_monitor_dpi_v2();

    // Второй экземпляр не нужен: оверлей и значок в трее были бы дублированы.
    if program::ensure_single_instance().is_err() {
        program::log("приложение уже запущено, второй экземпляр закрыт");
        return;
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = program::Command::parse(&args);

    match command {
        program::Command::ShowWindow { .. } => program::log("запуск окна утилиты"),
        program::Command::Apply(_) => {}
        program::Command::Revert { .. } => program::log("откат из командной строки"),
    }

    app::run(command);
    program::log("остановка");
}