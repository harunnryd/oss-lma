use std::{
    fmt,
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::{mpsc, Mutex},
    thread,
    time::Duration,
};

use serde::Serialize;

use crate::settings::{ProviderKind, ProviderSettings};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const EXTRA_READINESS_WINDOW: Duration = Duration::from_millis(30);

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
}

impl SidecarCommand {
    pub(crate) fn bundled() -> Self {
        Self::new("python3", ["-m", "sidecar"])
    }
    pub(crate) fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    fn start(&self) -> Result<Child, SupervisorError> {
        Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(SupervisorError::Spawn)
    }
}

pub(crate) struct SidecarSupervisor {
    command: SidecarCommand,
    state: Mutex<SupervisorState>,
}

struct SupervisorState {
    child: Option<Child>,
    endpoint: Option<SidecarEndpoint>,
    config: Option<RuntimeConfig>,
    last_error: Option<SupervisorError>,
}

#[derive(Debug)]
pub(crate) enum SupervisorError {
    Spawn(std::io::Error),
    Config(std::io::Error),
    InvalidReadiness,
    StartupTimeout,
    Shutdown(std::io::Error),
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
            }),
        }
    }

    pub(crate) fn spawn(&self, config: RuntimeConfig) -> Result<SidecarEndpoint, SupervisorError> {
        let mut child = self.command.start()?;
        forward_stderr(child.stderr.take());
        if let Err(error) = write_runtime_config(child.stdin.take(), &config) {
            terminate(&mut child);
            return Err(error);
        }
        let readiness = read_readiness(child.stdout.take())?;
        let endpoint = match readiness {
            Ok(endpoint) => endpoint,
            Err(error) => {
                terminate(&mut child);
                return Err(error);
            }
        };

        let mut state = self.state.lock().unwrap();
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
        let config = state.config.clone()?;
        drop(state);
        match self.spawn(config) {
            Ok(endpoint) => Some(endpoint),
            Err(error) => {
                self.state.lock().unwrap().last_error = Some(error);
                None
            }
        }
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
) -> Result<Result<SidecarEndpoint, SupervisorError>, SupervisorError> {
    let stdout = stdout.ok_or(SupervisorError::InvalidReadiness)?;
    let (sender, receiver) = mpsc::sync_channel(2);
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().take(2) {
            if sender.send(line).is_err() {
                return;
            }
        }
    });
    let first = receiver
        .recv_timeout(STARTUP_TIMEOUT)
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => SupervisorError::StartupTimeout,
            mpsc::RecvTimeoutError::Disconnected => SupervisorError::InvalidReadiness,
        })?
        .map_err(|_| SupervisorError::InvalidReadiness)?;
    let endpoint = parse_readiness(&first).ok_or(SupervisorError::InvalidReadiness)?;
    match receiver.recv_timeout(EXTRA_READINESS_WINDOW) {
        Ok(_) => Ok(Err(SupervisorError::InvalidReadiness)),
        Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {
            Ok(Ok(endpoint))
        }
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
    use std::time::Duration;

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

    fn shell(script: &str) -> SidecarCommand {
        SidecarCommand::new("sh", ["-c", script])
    }

    #[test]
    fn parses_exactly_one_ready_line_and_rejects_malformed_output() {
        let supervisor = SidecarSupervisor::with_command(shell(
            "printf 'SIDECAR_READY port=43123 token=abc123\\n'; cat >/dev/null",
        ));
        let endpoint = supervisor.spawn(config("provider-secret")).unwrap();
        assert_eq!(endpoint.port(), 43123);
        assert_eq!(endpoint.token().expose(), "abc123");
        supervisor.shutdown().unwrap();

        for script in [
            "printf 'ready port=43123 token=abc123\\n'; cat >/dev/null",
            "printf 'SIDECAR_READY port=43123 token=abc123\\nSIDECAR_READY port=43124 token=def456\\n'; cat >/dev/null",
        ] {
            let supervisor = SidecarSupervisor::with_command(shell(script));
            let error = match supervisor.spawn(config("provider-secret")) {
                Ok(_) => panic!("malformed readiness output must be rejected"),
                Err(error) => error,
            };
            assert!(matches!(error, SupervisorError::InvalidReadiness));
        }
    }

    #[test]
    fn shutdown_kills_child_and_clears_endpoint() {
        let supervisor = SidecarSupervisor::with_command(shell(
            "printf 'SIDECAR_READY port=43123 token=abc123\\n'; cat >/dev/null",
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
    fn respawn_replaces_endpoint_without_exposing_token() {
        let supervisor = SidecarSupervisor::with_command(shell(
            "cat >/dev/null & printf 'SIDECAR_READY port=43123 token=abc123\\n'; cat >/dev/null",
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
