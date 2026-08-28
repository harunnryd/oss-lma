use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            app::initialize_capture(app)
                .map_err(std::io::Error::other)
                .map_err(Into::into)
        })
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                app::shutdown_capture(window.app_handle());
            }
        })
        .invoke_handler(app::capture_invoke_handler())
        .run(tauri::generate_context!())
        .expect("desktop application failed");
}
