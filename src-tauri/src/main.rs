fn main() {
    tauri::Builder::default()
        .setup(|app| {
            app::initialize_capture(app)
                .map_err(std::io::Error::other)
                .map_err(Into::into)
        })
        .invoke_handler(app::capture_invoke_handler())
        .run(tauri::generate_context!())
        .expect("desktop application failed");
}
