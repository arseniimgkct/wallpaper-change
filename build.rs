fn main() {
    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        res.set_icon("app.ico");
        // Приложение всегда запускается с правами текущего пользователя,
        // поэтому манифест по умолчанию (asInvoker) подходит.
        res.set_manifest_file("app.manifest");
        if let Err(e) = res.compile() {
            eprintln!("winres: {e}");
        }
    }
    println!("cargo:rerun-if-changed=app.ico");
    println!("cargo:rerun-if-changed=app.manifest");
}