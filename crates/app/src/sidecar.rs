use std::{
    ffi::OsString,
    fmt,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{mpsc, Mutex},
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;

use crate::settings::{ProviderKind, ProviderSettings};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const RESPAWN_CAP: u32 = 5;
const RESPAWN_WINDOW: Duration = Duration::from_secs(60);
const RESPAWN_BACKOFF: Duration = Duration::from_millis(200);

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SecretString(String);

impl SecretString {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

#[derive(Clone)]
pub(crate) struct SidecarEndpoint {
    port: u16,
    token: SecretString,
}

impl SidecarEndpoint {
    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn token(&self) -> &SecretString {
        &self.token
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct RuntimeConfig {
    pub(crate) provider: ProviderKind,
    pub(crate) model: String,
    pub(crate) language: Option<String>,
    pub(crate) azure_region: Option<String>,
    pub(crate) api_key: SecretString,
}

#[derive(Clone)]
pub(crate) struct SidecarCommand {
    program: String,
    args: Vec<String>,
    pythonpath: Option<Vec<PathBuf>>,
    env: Vec<(OsString, OsString)>,
}

impl SidecarCommand {
    pub(crate) fn bundled() -> Self {
        if cfg!(debug_assertions) {
            let python_resources = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../python");
            return Self {
                pythonpath: Some(python_paths(python_resources)),
                ..Self::new("uv", ["run", "python", "-m", "sidecar"])
            };
        }
        let sidecar = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(PathBuf::from))
            .map(|executable_dir| {
                if cfg!(target_os = "macos") {
                    executable_dir.join("../Resources/sidecar/sidecar")
                } else if cfg!(target_os = "windows") {
                    executable_dir.join("resources/sidecar/sidecar.exe")
                } else {
                    executable_dir.join("resources/sidecar/sidecar")
                }
            });
        let program = sidecar
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "sidecar".to_owned());
        Self::new(program, std::iter::empty::<String>())
    }
    pub(crate) fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            pythonpath: None,
            env: Vec::new(),
        }
    }

    pub(crate) fn for_app_data_dir(app_data_dir: &Path) -> Self {
        Self::bundled()
            .with_env("LMA_DB_PATH", app_data_dir.join("lma.db"))
            .with_env("LMA_RECORDING_DIR", app_data_dir.join("recordings"))
    }

    fn with_env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    fn start(&self) -> Result<Child, SupervisorError> {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(paths) = &self.pythonpath {
            let pythonpath = std::env::join_paths(paths).map_err(|error| {
                SupervisorError::Spawn(std::io::Error::new(std::io::ErrorKind::InvalidInput, error))
            })?;
            command.env("PYTHONPATH", pythonpath);
        }
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command.spawn().map_err(SupervisorError::Spawn)
    }
}

fn python_paths(root: PathBuf) -> Vec<PathBuf> {
    vec![
        root.clone(),
        root.join("lma_stt"),
        root.join("lma_pipeline"),
    ]
}

pub(crate) struct SidecarSupervisor {
    command: SidecarCommand,
    state: Mutex<SupervisorState>,
}

struct SupervisorState {
    child: Option<Child>,
    endpoint: Option<SidecarEndpoint>,
    config: Option<RuntimeConfig>,
    last_error: Option<String>,
    respawn_count: u32,
    first_respawn_at: Option<Instant>,
    next_respawn_at: Option<Instant>,
}

#[derive(Debug)]
pub(crate) enum SupervisorError {
    Spawn(std::io::Error),
    Config(std::io::Error),
    InvalidReadiness,
    StartupTimeout,
    Shutdown(std::io::Error),
    RespawnLimitExceeded,
}

impl fmt::Display for SupervisorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(formatter, "sidecar could not start: {error}"),
            Self::Config(error) => write!(
                formatter,
                "sidecar configuration could not be sent: {error}"
            ),
            Self::InvalidReadiness => {
                formatter.write_str("sidecar emitted invalid readiness output")
            }
            Self::StartupTimeout => {
                formatter.write_str("sidecar did not become ready before startup timed out")
            }
            Self::Shutdown(error) => write!(formatter, "sidecar could not stop: {error}"),
            Self::RespawnLimitExceeded => {
                formatter.write_str("sidecar respawn limit exceeded; manual restart required")
            }
        }
    }
}

impl std::error::Error for SupervisorError {}

#[allow(dead_code)]
impl SidecarSupervisor {
    pub(crate) fn runtime_config(settings: ProviderSettings, api_key: String) -> RuntimeConfig {
        RuntimeConfig {
            provider: settings.provider,
            model: settings.model,
            language: settings.language,
            azure_region: settings.azure_region,
            api_key: SecretString::new(api_key),
        }
    }
    pub(crate) fn new(command: SidecarCommand) -> Self {
        Self::with_command(command)
    }

    pub(crate) fn with_command(command: SidecarCommand) -> Self {
        Self {
            command,
            state: Mutex::new(SupervisorState {
                child: None,
                endpoint: None,
                config: None,
                last_error: None,
                respawn_count: 0,
                first_respawn_at: None,
                next_respawn_at: None,
            }),
        }
    }

    pub(crate) fn spawn(&self, config: RuntimeConfig) -> Result<SidecarEndpoint, SupervisorError> {
        let backoff_target = {
            let state = self.state.lock().unwrap();
            state.next_respawn_at
        };
        if let Some(next_at) = backoff_target {
            if let Some(remaining) = next_at.checked_duration_since(Instant::now()) {
                std::thread::sleep(remaining);
            }
        }

        let mut state = self.state.lock().unwrap();
        let result = self.spawn_into_locked(&mut state, config);

        let now = Instant::now();
        match state.first_respawn_at {
            Some(first) if now.duration_since(first) <= RESPAWN_WINDOW => {
                state.respawn_count = state.respawn_count.saturating_add(1);
            }
            _ => {
                state.respawn_count = 1;
                state.first_respawn_at = Some(now);
            }
        }
        state.next_respawn_at = Some(now + RESPAWN_BACKOFF);
        if let Err(ref error) = result {
            state.last_error = Some(format!("{error}"));
        }
        if state.respawn_count > RESPAWN_CAP && result.is_err() {
            return Err(SupervisorError::RespawnLimitExceeded);
        }
        result
    }

    fn spawn_into_locked(
        &self,
        state: &mut SupervisorState,
        config: RuntimeConfig,
    ) -> Result<SidecarEndpoint, SupervisorError> {
        let mut child = self.command.start()?;
        forward_stderr(child.stderr.take());
        if let Err(error) = write_runtime_config(child.stdin.take(), &config) {
            terminate(&mut child);
            return Err(error);
        }
        let endpoint = match read_readiness(child.stdout.take()) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                terminate(&mut child);
                return Err(error);
            }
        };

        if let Some(mut previous) = state.child.take() {
            terminate(&mut previous);
        }
        state.endpoint = Some(endpoint.clone());
        state.child = Some(child);
        state.config = Some(config);
        state.last_error = None;
        Ok(endpoint)
    }

    pub(crate) fn endpoint(&self) -> Option<SidecarEndpoint> {
        let mut state = self.state.lock().unwrap();
        let exited = state
            .child
            .as_mut()
            .is_some_and(|child| child.try_wait().ok().flatten().is_some());
        if exited {
            state.child = None;
            state.endpoint = None;
        }
        if state.endpoint.is_some() {
            return state.endpoint.clone();
        }
        if state.respawn_count > RESPAWN_CAP {
            return None;
        }
        let config = state.config.as_ref()?.clone();
        let result = self.spawn_into_locked(&mut state, config);

        let now = Instant::now();
        match state.first_respawn_at {
            Some(first) if now.duration_since(first) <= RESPAWN_WINDOW => {
                state.respawn_count = state.respawn_count.saturating_add(1);
            }
            _ => {
                state.respawn_count = 1;
                state.first_respawn_at = Some(now);
            }
        }
        state.next_respawn_at = Some(now + RESPAWN_BACKOFF);
        if let Err(ref error) = result {
            state.last_error = Some(format!("{error}"));
        }
        result.ok()
    }

    pub(crate) fn respawn(
        &self,
        config: RuntimeConfig,
    ) -> Result<SidecarEndpoint, SupervisorError> {
        self.shutdown()?;
        self.spawn(config)
    }

    pub(crate) fn shutdown(&self) -> Result<(), SupervisorError> {
        let mut state = self.state.lock().unwrap();
        state.endpoint = None;
        state.config = None;
        state.respawn_count = 0;
        state.first_respawn_at = None;
        state.next_respawn_at = None;
        if let Some(mut child) = state.child.take() {
            let kill_error = child.kill().err();
            child.wait().map_err(SupervisorError::Shutdown)?;
            if let Some(error) = kill_error {
                return Err(SupervisorError::Shutdown(error));
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn child_has_exited(&self) -> bool {
        self.state
            .lock()
            .unwrap()
            .child
            .as_mut()
            .is_none_or(|child| child.try_wait().ok().flatten().is_some())
    }

    #[cfg(test)]
    fn command_description(&self) -> String {
        std::iter::once(self.command.program.as_str())
            .chain(self.command.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl Drop for SidecarSupervisor {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn write_runtime_config(
    stdin: Option<std::process::ChildStdin>,
    config: &RuntimeConfig,
) -> Result<(), SupervisorError> {
    let payload = serde_json::to_vec(&RuntimeConfigPayload::from(config))
        .map_err(|error| SupervisorError::Config(std::io::Error::other(error)))?;
    let length = u32::try_from(payload.len()).map_err(|_| {
        SupervisorError::Config(std::io::Error::other("runtime configuration is too large"))
    })?;
    let mut stdin = stdin.ok_or_else(|| {
        SupervisorError::Config(std::io::Error::other("sidecar stdin is unavailable"))
    })?;
    stdin
        .write_all(&length.to_be_bytes())
        .map_err(SupervisorError::Config)?;
    stdin.write_all(&payload).map_err(SupervisorError::Config)?;
    stdin.flush().map_err(SupervisorError::Config)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeConfigPayload<'a> {
    provider: ProviderKind,
    model: &'a str,
    language: &'a Option<String>,
    azure_region: &'a Option<String>,
    api_key: &'a str,
}

impl<'a> From<&'a RuntimeConfig> for RuntimeConfigPayload<'a> {
    fn from(config: &'a RuntimeConfig) -> Self {
        Self {
            provider: config.provider,
            model: &config.model,
            language: &config.language,
            azure_region: &config.azure_region,
            api_key: config.api_key.expose(),
        }
    }
}

fn read_readiness(
    stdout: Option<std::process::ChildStdout>,
) -> Result<SidecarEndpoint, SupervisorError> {
    let stdout = stdout.ok_or(SupervisorError::InvalidReadiness)?;
    let mut line = String::new();
    read_with_timeout(stdout, &mut line)?;
    parse_readiness(&line).ok_or(SupervisorError::InvalidReadiness)
}

fn read_with_timeout<R: Read + Send + 'static>(
    stream: R,
    buf: &mut String,
) -> Result<(), SupervisorError> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let result = reader.read_line(&mut line);
        let _ = sender.send(result.map(|_| line));
    });
    match receiver.recv_timeout(STARTUP_TIMEOUT) {
        Ok(Ok(line)) => {
            buf.push_str(&line);
            Ok(())
        }
        Ok(Err(_)) => Err(SupervisorError::InvalidReadiness),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(SupervisorError::StartupTimeout),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(SupervisorError::InvalidReadiness),
    }
}

fn parse_readiness(line: &str) -> Option<SidecarEndpoint> {
    let mut fields = line.split_whitespace();
    (fields.next()? == "SIDECAR_READY").then_some(())?;
    let port = fields.next()?.strip_prefix("port=")?.parse().ok()?;
    let token = fields.next()?.strip_prefix("token=")?;
    if fields.next().is_some()
        || token.is_empty()
        || !token.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some(SidecarEndpoint {
        port,
        token: SecretString::new(token),
    })
}

fn forward_stderr(stderr: Option<std::process::ChildStderr>) {
    if let Some(stderr) = stderr {
        thread::spawn(move || {
            for _ in BufReader::new(stderr).lines().map_while(Result::ok) {
                eprintln!("sidecar stderr event");
            }
        });
    }
}

fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use super::{RuntimeConfig, SecretString, SidecarCommand, SidecarSupervisor, SupervisorError};
    use crate::settings::ProviderKind;

    fn config(secret: &str) -> RuntimeConfig {
        RuntimeConfig {
            provider: ProviderKind::Deepgram,
            model: "nova-3".to_owned(),
            language: Some("en".to_owned()),
            azure_region: None,
            api_key: SecretString::new(secret),
        }
    }

    #[test]
    fn bundled_command_points_python_to_packaged_resources() {
        let command = SidecarCommand::bundled();
        assert_eq!(command.program, "uv");
        assert_eq!(command.args, ["run", "python", "-m", "sidecar"]);
        let pythonpaths = command
            .pythonpath
            .expect("bundled sidecar has Python resources");
        assert_eq!(pythonpaths.len(), 3);
        assert!(pythonpaths[0].ends_with("python"));
        assert!(pythonpaths[1].ends_with("python/lma_stt"));
        assert!(pythonpaths[2].ends_with("python/lma_pipeline"));
    }

    #[test]
    fn app_data_command_keeps_sidecar_storage_with_the_shell() {
        let app_data_dir = PathBuf::from("/tmp/oss-lma-app-data");
        let command = SidecarCommand::for_app_data_dir(&app_data_dir);
        assert!(command.env.iter().any(|(key, value)| {
            key == "LMA_DB_PATH" && value == "/tmp/oss-lma-app-data/lma.db"
        }));
        assert!(command.env.iter().any(|(key, value)| {
            key == "LMA_RECORDING_DIR" && value == "/tmp/oss-lma-app-data/recordings"
        }));
    }

    fn shell(script: &str) -> SidecarCommand {
        SidecarCommand::new(
            "sh",
            ["-c", &format!("{script}; exec 1>&-; cat >/dev/null")],
        )
    }

    #[test]
    fn parses_exactly_one_ready_line_and_rejects_malformed_output() {
        let supervisor = SidecarSupervisor::with_command(shell(
            "printf 'SIDECAR_READY port=43123 token=abc123\\n'",
        ));
        let endpoint = supervisor.spawn(config("provider-secret")).unwrap();
        assert_eq!(endpoint.port(), 43123);
        assert_eq!(endpoint.token().expose(), "abc123");
        supervisor.shutdown().unwrap();

        let supervisor =
            SidecarSupervisor::with_command(shell("printf 'ready port=43123 token=abc123\\n'"));
        let error = match supervisor.spawn(config("provider-secret")) {
            Ok(_) => panic!("malformed readiness output must be rejected"),
            Err(error) => error,
        };
        assert!(matches!(error, SupervisorError::InvalidReadiness));
    }

    #[test]
    fn shutdown_kills_child_and_clears_endpoint() {
        let supervisor = SidecarSupervisor::with_command(shell(
            "printf 'SIDECAR_READY port=43123 token=abc123\\n'",
        ));
        supervisor.spawn(config("provider-secret")).unwrap();

        supervisor.shutdown().unwrap();

        assert!(supervisor.endpoint().is_none());
        assert!(supervisor.child_has_exited());
    }

    #[test]
    fn shutdown_reaps_a_child_that_exited_before_kill() {
        let supervisor = SidecarSupervisor::with_command(shell(
            "printf 'SIDECAR_READY port=43123 token=abc123\\n'; exit 0",
        ));
        supervisor.spawn(config("provider-secret")).unwrap();

        supervisor.shutdown().unwrap();

        assert!(supervisor.endpoint().is_none());
    }

    #[test]
    fn endpoint_respawns_after_the_child_exits() {
        let supervisor = SidecarSupervisor::with_command(shell(
            "printf 'SIDECAR_READY port=43123 token=abc123\\n'; exit 0",
        ));
        supervisor.spawn(config("provider-secret")).unwrap();

        std::thread::sleep(Duration::from_millis(10));
        let endpoint = supervisor.endpoint().expect("sidecar is respawned");

        assert_eq!(endpoint.port(), 43123);
        supervisor.shutdown().unwrap();
    }

    #[test]
    fn endpoint_caps_respawn_attempts_after_repeated_failures() {
        let supervisor = SidecarSupervisor::with_command(shell(
            "printf 'SIDECAR_READY port=43123 token=abc123\n'; exit 0",
        ));
        supervisor.spawn(config("provider-secret")).unwrap();

        for _ in 0..5 {
            std::thread::sleep(Duration::from_millis(20));
            assert!(
                supervisor.endpoint().is_some(),
                "first five attempts succeed"
            );
        }
        std::thread::sleep(Duration::from_millis(20));
        assert!(
            supervisor.endpoint().is_none(),
            "sixth attempt within the window is capped"
        );
        supervisor.shutdown().unwrap();
    }

    #[test]
    fn shutdown_resets_the_respawn_cap() {
        let supervisor = SidecarSupervisor::with_command(shell(
            "printf 'SIDECAR_READY port=43123 token=abc123\n'; exit 0",
        ));
        supervisor.spawn(config("provider-secret")).unwrap();
        for _ in 0..5 {
            std::thread::sleep(Duration::from_millis(20));
            supervisor.endpoint();
        }
        std::thread::sleep(Duration::from_millis(20));
        assert!(supervisor.endpoint().is_none(), "cap was reached");

        supervisor.shutdown().unwrap();

        let endpoint = supervisor.spawn(config("fresh-secret")).unwrap();
        assert_eq!(endpoint.port(), 43123);
        supervisor.shutdown().unwrap();
    }

    #[test]
    fn spawn_rejects_when_cap_reached() {
        let supervisor =
            SidecarSupervisor::with_command(shell("printf 'unrelated output\n'; exit 1"));
        for _ in 0..5 {
            let _ = supervisor.spawn(config("provider-secret"));
        }
        let result = supervisor.spawn(config("provider-secret"));
        assert!(matches!(result, Err(SupervisorError::RespawnLimitExceeded)));
    }

    #[test]
    fn respawn_replaces_endpoint_without_exposing_token() {
        let supervisor = SidecarSupervisor::with_command(shell(
            "printf 'SIDECAR_READY port=43123 token=abc123\\n'",
        ));
        supervisor.spawn(config("first-provider-secret")).unwrap();

        let endpoint = supervisor
            .respawn(config("second-provider-secret"))
            .unwrap();

        assert_eq!(endpoint.port(), 43123);
        assert_eq!(endpoint.token().expose(), "abc123");
        assert!(!supervisor
            .command_description()
            .contains("first-provider-secret"));
        assert!(!supervisor
            .command_description()
            .contains("second-provider-secret"));
        supervisor.shutdown().unwrap();
    }
}
