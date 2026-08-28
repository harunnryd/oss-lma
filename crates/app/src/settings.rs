use std::{
    collections::HashMap,
    fmt,
    path::{Path, PathBuf},
    sync::Mutex,
};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

const PROVIDER_SETTINGS_KEY: &str = "stt.provider_settings";
const KEYCHAIN_SERVICE: &str = "com.oss-lma.desktop";

#[derive(Clone, Copy, Debug, Deserialize, Hash, PartialEq, Eq, Serialize)]
pub enum ProviderKind {
    #[serde(rename = "deepgram")]
    Deepgram,
    #[serde(rename = "assemblyAi")]
    AssemblyAi,
    #[serde(rename = "azure")]
    Azure,
}

impl ProviderKind {
    fn keychain_account(self) -> &'static str {
        match self {
            Self::Deepgram => "stt-provider-deepgram",
            Self::AssemblyAi => "stt-provider-assembly-ai",
            Self::Azure => "stt-provider-azure",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettings {
    pub provider: ProviderKind,
    pub model: String,
    pub language: Option<String>,
    pub azure_region: Option<String>,
    pub diarize_system: bool,
    pub diarize_mic: bool,
}

impl Default for ProviderSettings {
    fn default() -> Self {
        Self {
            provider: ProviderKind::Deepgram,
            model: "nova-3".to_owned(),
            language: None,
            azure_region: None,
            diarize_system: false,
            diarize_mic: false,
        }
    }
}

impl ProviderSettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.model.trim().is_empty() {
            return Err(SettingsError::InvalidSettings(
                "a provider model is required".to_owned(),
            ));
        }
        if self.provider == ProviderKind::Azure
            && self
                .azure_region
                .as_deref()
                .is_none_or(|region| region.trim().is_empty())
        {
            return Err(SettingsError::InvalidSettings(
                "an Azure region is required for the Azure provider".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPublicSettings {
    pub provider: ProviderKind,
    pub model: String,
    pub language: Option<String>,
    pub azure_region: Option<String>,
    pub diarize_system: bool,
    pub diarize_mic: bool,
    pub has_secret: bool,
}

impl ProviderPublicSettings {
    pub fn from_settings(settings: &ProviderSettings, has_secret: bool) -> Self {
        Self {
            provider: settings.provider,
            model: settings.model.clone(),
            language: settings.language.clone(),
            azure_region: settings.azure_region.clone(),
            diarize_system: settings.diarize_system,
            diarize_mic: settings.diarize_mic,
            has_secret,
        }
    }
}

#[derive(Debug)]
pub enum SettingsError {
    InvalidSettings(String),
    MissingSecret(ProviderKind),
    Storage(String),
    SecretStore(String),
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSettings(message)
            | Self::Storage(message)
            | Self::SecretStore(message) => formatter.write_str(message),
            Self::MissingSecret(provider) => {
                write!(formatter, "no secret is stored for {provider:?}")
            }
        }
    }
}

impl std::error::Error for SettingsError {}

impl From<rusqlite::Error> for SettingsError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

pub trait SecretStore: Send + Sync {
    fn set(&self, provider: ProviderKind, secret: &str) -> Result<(), SettingsError>;
    fn get(&self, provider: ProviderKind) -> Result<String, SettingsError>;
    fn delete(&self, provider: ProviderKind) -> Result<(), SettingsError>;
    fn has(&self, provider: ProviderKind) -> bool;

    fn replace(&self, provider: ProviderKind, secret: &str) -> Result<(), SettingsError> {
        match self.delete(provider) {
            Ok(()) | Err(SettingsError::MissingSecret(_)) => self.set(provider, secret),
            Err(error) => Err(error),
        }
    }
}

#[derive(Default)]
pub struct InMemorySecretStore {
    secrets: Mutex<HashMap<ProviderKind, String>>,
}

impl SecretStore for InMemorySecretStore {
    fn set(&self, provider: ProviderKind, secret: &str) -> Result<(), SettingsError> {
        self.secrets
            .lock()
            .unwrap()
            .insert(provider, secret.to_owned());
        Ok(())
    }

    fn get(&self, provider: ProviderKind) -> Result<String, SettingsError> {
        self.secrets
            .lock()
            .unwrap()
            .get(&provider)
            .cloned()
            .ok_or(SettingsError::MissingSecret(provider))
    }

    fn delete(&self, provider: ProviderKind) -> Result<(), SettingsError> {
        self.secrets.lock().unwrap().remove(&provider);
        Ok(())
    }

    fn has(&self, provider: ProviderKind) -> bool {
        self.secrets.lock().unwrap().contains_key(&provider)
    }
}

pub struct FileSecretStore {
    directory: PathBuf,
}

impl FileSecretStore {
    pub fn new(directory: PathBuf) -> Result<Self, SettingsError> {
        std::fs::create_dir_all(&directory)
            .map_err(|error| SettingsError::Storage(error.to_string()))?;
        Ok(Self { directory })
    }

    fn path_for(&self, provider: ProviderKind) -> PathBuf {
        self.directory
            .join(format!("{}.key", provider.keychain_account()))
    }
}

impl SecretStore for FileSecretStore {
    fn set(&self, provider: ProviderKind, secret: &str) -> Result<(), SettingsError> {
        let path = self.path_for(provider);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .map_err(|error| SettingsError::Storage(error.to_string()))?;
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&path, secret)
                .map_err(|error| SettingsError::Storage(error.to_string()))?;
        }
        std::fs::write(&path, secret).map_err(|error| SettingsError::Storage(error.to_string()))?;
        Ok(())
    }

    fn get(&self, provider: ProviderKind) -> Result<String, SettingsError> {
        let path = self.path_for(provider);
        match std::fs::read_to_string(&path) {
            Ok(secret) => Ok(secret),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(SettingsError::MissingSecret(provider))
            }
            Err(error) => Err(SettingsError::Storage(error.to_string())),
        }
    }

    fn delete(&self, provider: ProviderKind) -> Result<(), SettingsError> {
        let path = self.path_for(provider);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(SettingsError::MissingSecret(provider))
            }
            Err(error) => Err(SettingsError::Storage(error.to_string())),
        }
    }

    fn has(&self, provider: ProviderKind) -> bool {
        self.path_for(provider).exists()
    }
}

pub fn pick_secret_store(app_data_dir: &Path) -> Box<dyn SecretStore> {
    pick_secret_store_with_probe(app_data_dir, || OsSecretStore.get(ProviderKind::Deepgram))
}

pub(crate) fn pick_secret_store_with_probe<F>(app_data_dir: &Path, probe: F) -> Box<dyn SecretStore>
where
    F: FnOnce() -> Result<String, SettingsError>,
{
    match probe() {
        Err(SettingsError::MissingSecret(_)) => {
            eprintln!("[oss-lma] secrets stored in OS keychain (service: {KEYCHAIN_SERVICE})");
            Box::new(OsSecretStore)
        }
        Err(error) => {
            eprintln!(
                "[oss-lma] OS keychain unavailable ({error}); falling back to file-based store under {}",
                app_data_dir.join("secrets").display()
            );
            Box::new(
                FileSecretStore::new(app_data_dir.join("secrets"))
                    .expect("file secret store directory is creatable"),
            )
        }
        Ok(_) => {
            eprintln!("[oss-lma] secrets stored in OS keychain (service: {KEYCHAIN_SERVICE})");
            Box::new(OsSecretStore)
        }
    }
}

pub struct OsSecretStore;

impl SecretStore for OsSecretStore {
    fn set(&self, provider: ProviderKind, secret: &str) -> Result<(), SettingsError> {
        platform::set(provider, secret)
    }

    fn get(&self, provider: ProviderKind) -> Result<String, SettingsError> {
        platform::get(provider)
    }

    fn delete(&self, provider: ProviderKind) -> Result<(), SettingsError> {
        platform::delete(provider)
    }

    fn has(&self, provider: ProviderKind) -> bool {
        self.get(provider).is_ok()
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod platform {
    use std::sync::mpsc;
    use std::time::Duration;

    use keyring::Entry;

    use super::{ProviderKind, SettingsError, KEYCHAIN_SERVICE};

    fn entry(provider: ProviderKind) -> Result<Entry, SettingsError> {
        Entry::new(KEYCHAIN_SERVICE, provider.keychain_account())
            .map_err(|error| SettingsError::SecretStore(error.to_string()))
    }

    pub(super) fn set(provider: ProviderKind, secret: &str) -> Result<(), SettingsError> {
        entry(provider)?
            .set_password(secret)
            .map_err(|error| SettingsError::SecretStore(error.to_string()))
    }

    pub(super) fn get(provider: ProviderKind) -> Result<String, SettingsError> {
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = match entry(provider) {
                Ok(credential) => match credential.get_password() {
                    Ok(secret) => Ok(secret),
                    Err(keyring::Error::NoEntry) => Err(SettingsError::MissingSecret(provider)),
                    Err(error) => Err(SettingsError::SecretStore(error.to_string())),
                },
                Err(error) => Err(error),
            };
            let _ = sender.send(result);
        });
        receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| SettingsError::SecretStore("OS keychain lookup timed out".to_owned()))?
    }

    pub(super) fn delete(provider: ProviderKind) -> Result<(), SettingsError> {
        match entry(provider)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Err(SettingsError::MissingSecret(provider)),
            Err(error) => Err(SettingsError::SecretStore(error.to_string())),
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod platform {
    use super::{ProviderKind, SettingsError};

    fn unsupported() -> SettingsError {
        SettingsError::SecretStore("OS keychain is unsupported on this platform".to_owned())
    }

    pub(super) fn set(_provider: ProviderKind, _secret: &str) -> Result<(), SettingsError> {
        Err(unsupported())
    }

    pub(super) fn get(_provider: ProviderKind) -> Result<String, SettingsError> {
        Err(unsupported())
    }

    pub(super) fn delete(_provider: ProviderKind) -> Result<(), SettingsError> {
        Err(unsupported())
    }
}

pub struct SettingsRepository {
    connection: Mutex<Connection>,
}

impl SettingsRepository {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SettingsError> {
        Self::from_connection(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self, SettingsError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self, SettingsError> {
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn load(&self) -> Result<ProviderSettings, SettingsError> {
        let connection = self.connection.lock().unwrap();
        let value: Option<String> = connection
            .query_row(
                "SELECT value_json FROM settings WHERE key = ?1",
                [PROVIDER_SETTINGS_KEY],
                |row| row.get(0),
            )
            .optional()?;
        match value {
            Some(value) => {
                let settings: ProviderSettings = serde_json::from_str(&value)
                    .map_err(|error| SettingsError::Storage(error.to_string()))?;
                settings.validate()?;
                Ok(settings)
            }
            None => Ok(ProviderSettings::default()),
        }
    }

    pub fn save(&self, settings: &ProviderSettings) -> Result<(), SettingsError> {
        settings.validate()?;
        let public_settings = serde_json::to_string(settings)
            .map_err(|error| SettingsError::Storage(error.to_string()))?;
        self.connection.lock().unwrap().execute(
            "INSERT INTO settings (key, value_json) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
            params![PROVIDER_SETTINGS_KEY, public_settings],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use rusqlite::{params, Connection};
    use serde_json::Value;

    use super::{
        pick_secret_store_with_probe, FileSecretStore, InMemorySecretStore, ProviderKind,
        ProviderPublicSettings, ProviderSettings, SecretStore, SettingsError, SettingsRepository,
    };

    #[derive(Default)]
    struct DuplicateRejectingSecretStore {
        secrets: Mutex<HashMap<ProviderKind, String>>,
    }

    impl SecretStore for DuplicateRejectingSecretStore {
        fn set(&self, provider: ProviderKind, secret: &str) -> Result<(), SettingsError> {
            let mut secrets = self.secrets.lock().unwrap();
            if secrets.contains_key(&provider) {
                return Err(SettingsError::SecretStore(
                    "credential already exists".to_owned(),
                ));
            }
            secrets.insert(provider, secret.to_owned());
            Ok(())
        }

        fn get(&self, provider: ProviderKind) -> Result<String, SettingsError> {
            self.secrets
                .lock()
                .unwrap()
                .get(&provider)
                .cloned()
                .ok_or(SettingsError::MissingSecret(provider))
        }

        fn delete(&self, provider: ProviderKind) -> Result<(), SettingsError> {
            self.secrets
                .lock()
                .unwrap()
                .remove(&provider)
                .map(|_| ())
                .ok_or(SettingsError::MissingSecret(provider))
        }

        fn has(&self, provider: ProviderKind) -> bool {
            self.secrets.lock().unwrap().contains_key(&provider)
        }
    }

    fn deepgram_settings() -> ProviderSettings {
        ProviderSettings {
            provider: ProviderKind::Deepgram,
            model: "nova-3".to_owned(),
            language: Some("en".to_owned()),
            azure_region: None,
            diarize_system: true,
            diarize_mic: false,
        }
    }

    fn repository() -> SettingsRepository {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE settings (
                    key TEXT PRIMARY KEY,
                    value_json TEXT NOT NULL
                )",
            )
            .unwrap();
        SettingsRepository::from_connection(connection).unwrap()
    }

    #[test]
    fn public_settings_round_trip_without_secret_field() {
        let repository = repository();
        let settings = deepgram_settings();

        repository.save(&settings).unwrap();
        let reloaded = repository.load().unwrap();
        let public = ProviderPublicSettings::from_settings(&reloaded, false);
        let json = serde_json::to_value(public).unwrap();

        assert_eq!(reloaded, settings);
        assert_eq!(json["provider"], Value::String("deepgram".to_owned()));
        assert_eq!(json["hasSecret"], Value::Bool(false));
        assert!(json.get("secret").is_none());
        assert!(json.get("apiKey").is_none());
    }

    #[test]
    fn provider_secret_is_available_only_through_secret_store() {
        let secrets = InMemorySecretStore::default();
        let provider = ProviderKind::Deepgram;

        assert!(!secrets.has(provider));
        secrets.set(provider, "test-secret").unwrap();
        assert!(secrets.has(provider));
        assert_eq!(secrets.get(provider).unwrap(), "test-secret");
        secrets.delete(provider).unwrap();
        assert!(!secrets.has(provider));
    }

    #[test]
    fn replacing_an_existing_secret_works_when_the_store_rejects_duplicates() {
        let secrets = DuplicateRejectingSecretStore::default();
        let provider = ProviderKind::Deepgram;

        secrets.set(provider, "old-secret").unwrap();
        secrets.replace(provider, "new-secret").unwrap();

        assert_eq!(secrets.get(provider).unwrap(), "new-secret");
    }

    #[test]
    fn invalid_provider_settings_are_rejected_before_start() {
        let repository = repository();
        let empty_model = ProviderSettings {
            model: " ".to_owned(),
            ..deepgram_settings()
        };
        let missing_azure_region = ProviderSettings {
            provider: ProviderKind::Azure,
            model: "standard".to_owned(),
            ..deepgram_settings()
        };

        assert!(repository.save(&empty_model).is_err());
        assert!(repository.save(&missing_azure_region).is_err());
    }

    #[test]
    fn load_rejects_invalid_stored_provider_settings() {
        let repository = repository();
        let invalid_settings = ProviderSettings {
            model: " ".to_owned(),
            ..deepgram_settings()
        };
        repository
            .connection
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO settings (key, value_json) VALUES (?1, ?2)",
                params![
                    super::PROVIDER_SETTINGS_KEY,
                    serde_json::to_string(&invalid_settings).unwrap()
                ],
            )
            .unwrap();

        assert!(matches!(
            repository.load(),
            Err(super::SettingsError::InvalidSettings(_))
        ));
    }

    #[test]
    fn file_secret_store_round_trips_and_enforces_permissions() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = FileSecretStore::new(temp.path().to_path_buf()).expect("init");

        assert!(!store.has(ProviderKind::Deepgram));
        assert!(matches!(
            store.get(ProviderKind::Deepgram),
            Err(SettingsError::MissingSecret(ProviderKind::Deepgram))
        ));

        store
            .set(ProviderKind::Deepgram, "fake-secret")
            .expect("set should succeed");
        assert!(store.has(ProviderKind::Deepgram));
        assert_eq!(store.get(ProviderKind::Deepgram).unwrap(), "fake-secret");

        store
            .replace(ProviderKind::Deepgram, "rotated-secret")
            .expect("replace should succeed");
        assert_eq!(store.get(ProviderKind::Deepgram).unwrap(), "rotated-secret");

        store
            .delete(ProviderKind::Deepgram)
            .expect("delete should succeed");
        assert!(!store.has(ProviderKind::Deepgram));
    }

    #[cfg(unix)]
    #[test]
    fn file_secret_store_files_have_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("tempdir");
        let store = FileSecretStore::new(temp.path().to_path_buf()).expect("init");
        store.set(ProviderKind::Azure, "azure-secret").unwrap();
        let path = store.path_for(ProviderKind::Azure);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "expected 0600, got {mode:o}");
    }

    #[test]
    fn file_secret_store_isolates_providers() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = FileSecretStore::new(temp.path().to_path_buf()).expect("init");
        store.set(ProviderKind::Deepgram, "dg-secret").unwrap();
        store.set(ProviderKind::AssemblyAi, "aa-secret").unwrap();
        store.set(ProviderKind::Azure, "az-secret").unwrap();
        assert_eq!(store.get(ProviderKind::Deepgram).unwrap(), "dg-secret");
        assert_eq!(store.get(ProviderKind::AssemblyAi).unwrap(), "aa-secret");
        assert_eq!(store.get(ProviderKind::Azure).unwrap(), "az-secret");
        store.delete(ProviderKind::AssemblyAi).unwrap();
        assert!(!store.has(ProviderKind::AssemblyAi));
        assert!(store.has(ProviderKind::Deepgram));
        assert!(store.has(ProviderKind::Azure));
    }

    #[test]
    fn pick_secret_store_prefers_keychain_when_probe_succeeds() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _store = pick_secret_store_with_probe(temp.path(), || {
            Err(SettingsError::MissingSecret(ProviderKind::Deepgram))
        });
        assert!(!temp.path().join("secrets").exists());
    }

    #[test]
    fn pick_secret_store_falls_back_to_file_when_keychain_denies_access() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = pick_secret_store_with_probe(temp.path(), || {
            Err(SettingsError::SecretStore(
                "NoStorageAccess(test): security framework refused access".to_owned(),
            ))
        });
        store.set(ProviderKind::Deepgram, "ok").unwrap();
        assert!(store.has(ProviderKind::Deepgram));
        assert_eq!(store.get(ProviderKind::Deepgram).unwrap(), "ok");
    }

    #[test]
    fn provider_kind_serialises_lowercase_for_sidecar_wire() {
        for (variant, expected) in [
            (ProviderKind::Deepgram, "\"deepgram\""),
            (ProviderKind::AssemblyAi, "\"assemblyAi\""),
            (ProviderKind::Azure, "\"azure\""),
        ] {
            let payload = serde_json::to_string(&variant).expect("serialise");
            assert!(
                payload.contains(expected),
                "expected payload to contain {expected}, got {payload}"
            );
            assert!(
                !payload.contains("Deepgram")
                    && !payload.contains("AssemblyAi")
                    && !payload.contains("Azure"),
                "serialised provider must not leak PascalCase variants: {payload}"
            );
        }
    }
}
