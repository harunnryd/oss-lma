pub mod capture_state;
pub mod settings;
pub(crate) mod sidecar;
pub mod commands {
    pub mod capture;
}

use std::sync::Arc;

use tauri::{Manager, Runtime};

use crate::sidecar::{SidecarCommand, SidecarSupervisor};

pub fn initialize_capture<R: Runtime>(app: &tauri::App<R>) -> Result<(), String> {
    let sidecar = Arc::new(SidecarSupervisor::new(SidecarCommand::bundled()));
    if !app.manage(sidecar.clone()) {
        return Err("sidecar supervisor has already been initialized".to_owned());
    }
    let capture = commands::capture::AppCapture::from_tauri(app.handle(), sidecar)?;
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
