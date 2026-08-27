use tauri::State;

use crate::{
    settings::{OsSecretStore, ProviderPublicSettings, ProviderSettings, SecretStore},
    sidecar::SidecarSupervisor,
    ProviderSettingsState,
};

#[tauri::command]
pub fn provider_settings(
    state: State<'_, ProviderSettingsState>,
) -> Result<ProviderPublicSettings, String> {
    let settings = state.repository.load().map_err(|error| error.to_string())?;
    Ok(ProviderPublicSettings::from_settings(
        &settings,
        OsSecretStore.has(settings.provider),
    ))
}

#[tauri::command]
pub fn save_provider_settings(
    settings: ProviderSettings,
    secret: Option<String>,
    state: State<'_, ProviderSettingsState>,
) -> Result<ProviderPublicSettings, String> {
    settings.validate().map_err(|error| error.to_string())?;
    if let Some(secret) = secret {
        if secret.trim().is_empty() {
            return Err("the provider API key cannot be empty".to_owned());
        }
        OsSecretStore
            .replace(settings.provider, &secret)
            .map_err(|error| error.to_string())?;
    }
    state
        .repository
        .save(&settings)
        .map_err(|error| error.to_string())?;
    let has_secret = OsSecretStore.has(settings.provider);
    if has_secret {
        let secret = OsSecretStore
            .get(settings.provider)
            .map_err(|error| error.to_string())?;
        state
            .sidecar
            .respawn(SidecarSupervisor::runtime_config(settings.clone(), secret))
            .map_err(|error| error.to_string())?;
    }
    Ok(ProviderPublicSettings::from_settings(&settings, has_secret))
}
