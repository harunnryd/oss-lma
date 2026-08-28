pub mod capture_state;
pub mod meetings;
pub mod settings;
pub(crate) mod sidecar;
pub mod commands {
    pub mod capture;
    pub mod meetings;
    pub mod settings;
}

use std::sync::Arc;

use tauri::{Manager, Runtime};

use crate::{
    settings::{pick_secret_store, SecretStore, SettingsRepository},
    sidecar::{SidecarCommand, SidecarSupervisor},
};

pub struct ProviderSettingsState {
    repository: SettingsRepository,
    secret_store: Box<dyn SecretStore>,
    sidecar: Arc<SidecarSupervisor>,
}

pub fn initialize_capture<R: Runtime>(app: &tauri::App<R>) -> Result<(), String> {
    let sidecar = Arc::new(SidecarSupervisor::new(SidecarCommand::bundled()));
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&app_data_dir).map_err(|error| error.to_string())?;
    let settings_path = app_data_dir.join("settings.sqlite");
    rusqlite::Connection::open(&settings_path)
        .and_then(|connection| {
            connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value_json TEXT NOT NULL)",
            )
        })
        .map_err(|error| error.to_string())?;
    let repository = SettingsRepository::open(&settings_path).map_err(|error| error.to_string())?;
    let settings = repository.load().map_err(|error| error.to_string())?;
    let secret_store: Box<dyn SecretStore> = pick_secret_store(&app_data_dir);
    if let Ok(api_key) = secret_store.get(settings.provider) {
        sidecar
            .spawn(SidecarSupervisor::runtime_config(settings, api_key))
            .map_err(|error| error.to_string())?;
    }
    if !app.manage(sidecar.clone()) {
        return Err("sidecar supervisor has already been initialized".to_owned());
    }
    if !app.manage(ProviderSettingsState {
        repository,
        secret_store,
        sidecar: sidecar.clone(),
    }) {
        return Err("provider settings state has already been initialized".to_owned());
    }
    let capture = commands::capture::AppCapture::from_tauri(app.handle(), sidecar)?;
    let meetings_state = commands::meetings::MeetingsState::from_tauri(app)?;
    if app.manage(capture) {
        if !app.manage(meetings_state) {
            return Err("meetings state has already been initialized".to_owned());
        }
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
        commands::meetings::list_meetings,
        commands::meetings::list_meeting_segments,
        commands::meetings::delete_meeting,
        commands::settings::provider_settings,
        commands::settings::save_provider_settings,
    ]
}
