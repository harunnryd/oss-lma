pub mod capture_state;
pub mod settings;
pub mod commands {
    pub mod capture;
}

use tauri::{Manager, Runtime};

pub fn initialize_capture<R: Runtime>(app: &tauri::App<R>) -> Result<(), String> {
    let capture = commands::capture::AppCapture::from_tauri(app.handle())?;
    if app.manage(capture) {
        Ok(())
    } else {
        Err("capture state has already been initialized".to_owned())
    }
}

pub fn capture_invoke_handler<R: Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync
{
    tauri::generate_handler![
        commands::capture::capture_permissions,
        commands::capture::open_capture_permission_settings,
        commands::capture::capture_devices,
        commands::capture::set_capture_devices,
        commands::capture::start_meeting,
        commands::capture::pause_meeting,
        commands::capture::resume_meeting,
        commands::capture::stop_meeting,
        commands::capture::capture_status,
    ]
}
