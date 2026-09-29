use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::credentials::{CredentialStore, SecretString};
use crate::runtime::{
    ManagedProcessSpec, ProcessSnapshot, StopDisposition, SupervisorError, WindowsProcessSupervisor,
};

use super::bundle::{VerifiedTunnelBundle, verify_bundle};
use super::config::TunnelRuntimeConfig;
use super::fault::{Retryability, TunnelError};
use super::health::{ConnectorEndpoint, HealthEndpoint};

const API_KEY_ENV: &str = "LOCALBRIDGE_RUNTIME_API_KEY";
const MCP_GUARD_BEARER_ENV: &str = "LOCALBRIDGE_MCP_GUARD_BEARER";
const TUNNEL_ID_ENV: &str = "CONTROL_PLANE_TUNNEL_ID";
const API_KEY_REFERENCE: &str = "env:LOCALBRIDGE_RUNTIME_API_KEY";
static HEALTH_GENERATION: AtomicU64 = AtomicU64::new(1);

const REMOVED_PARENT_ENV: &[&str] = &[
    "CONTROL_PLANE_API_KEY",
    "OPENAI_API_KEY",
    "CONTROL_PLANE_URL_PATH",
    "CONTROL_PLANE_ORGANIZATION_ID",
    "CONTROL_PLANE_HTTP_PROXY",
    "CONTROL_PLANE_MAX_INFLIGHT_REQUESTS",
    "CONTROL_PLANE_POLL_CHANNELS",
    "CONTROL_PLANE_POLL_DEADLINE_GUARDRAIL",
    "CONTROL_PLANE_POLL_TIMEOUT",
    "CONTROL_PLANE_EXTRA_HEADERS",
    "CONTROL_PLANE_CLIENT_CERT",
    "CONTROL_PLANE_CLIENT_KEY",
    "TUNNEL_CLIENT_CONFIG",
    "TUNNEL_CLIENT_PROFILE",
    "TUNNEL_CLIENT_PROFILE_FILE",
    "TUNNEL_CLIENT_PROFILE_DIR",
    "XDG_CONFIG_HOME",
    "CA_BUNDLE",
    "HEALTH_UNIX_SOCKET",
    "HEALTH_URL_FILE",
    "MCP_COMMAND",
    "MCP_SERVER_URL",
    "MCP_HTTP_PROXY",
    "MCP_CONNECTION_MAX_TTL",
    "MCP_MAX_CONCURRENT_REQUESTS",
    "MCP_EXTRA_HEADERS",
    "MCP_DISCOVERY_EXTRA_HEADERS",
    "MCP_CLIENT_CERT",
    "MCP_CLIENT_KEY",
    "HARPOON_TARGETS",
    "HARPOON_ADDITIONAL_TRANSPORTS",
    "HARPOON_ALLOW_PLAINTEXT_HTTP",
    "HARPOON_CAPTURE_PAYLOADS",
    "HARPOON_HOSTS_INCLUDE_LOOPBACK",
    "HARPOON_HOSTS_INCLUDE_PRIVATE",
    "HARPOON_HOSTS_INCLUDE_REGEX",
    "HARPOON_HOSTS_INCLUDE_SUFFIX",
    "HARPOON_HTTP_PROXY",
    "HARPOON_MAX_REDIRECTS",
    "HARPOON_MAX_RESPONSE_BYTES",
    "CLOUDFLARED_TUNNEL_TOKEN",
    "CLOUDFLARED_MANAGED",
    "CLOUDFLARED_PATH",
    "CLOUDFLARED_READY_TIMEOUT",
    "ALLOW_REMOTE_UI",
    "OPEN_WEB_UI",
    "ADMIN_UI_LOG_BUFFER_EVENTS",
    "PID_FILE",
    "LOG_HTTP_RAW_UNSAFE",
    "LOG_FILE",
];

pub struct PreparedTunnelStart {
    config: TunnelRuntimeConfig,
    bundle: VerifiedTunnelBundle,
    secret: SecretString,
    mcp_guard_bearer: Option<SecretString>,
    health_url_file: PathBuf,
    #[cfg(test)]
    isolate_probe_environment: bool,
}

impl fmt::Debug for PreparedTunnelStart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedTunnelStart")
            .field("config", &self.config)
            .field("bundle", &self.bundle)
            .field("secret", &"[REDACTED]")
            .field("mcp_guard_authenticated", &self.mcp_guard_bearer.is_some())
            .field("health_url_file", &self.health_url_file)
            .field("arguments", &self.command_line_arguments())
            .finish()
    }
}

impl PreparedTunnelStart {
    pub fn prepare<C: CredentialStore>(
        config: TunnelRuntimeConfig,
        store: &C,
    ) -> Result<Self, TunnelError> {
        let bundle = verify_bundle(&config.install_root)?;
        let secret = store
            .read_runtime_api_key()
            .map_err(TunnelError::SecretStoreFailed)?
            .ok_or(TunnelError::RuntimeKeyMissing)?;
        fs::create_dir_all(&config.health_state_dir).map_err(|_| TunnelError::HealthStateIo)?;
        let generation = HEALTH_GENERATION.fetch_add(1, Ordering::Relaxed);
        let health_url_file = config.health_state_dir.join(format!(
            "tunnel-health-{}-{generation}.url",
            std::process::id()
        ));
        if health_url_file.exists() {
            fs::remove_file(&health_url_file).map_err(|_| TunnelError::HealthStateIo)?;
        }
        Ok(Self {
            config,
            bundle,
            secret,
            mcp_guard_bearer: None,
            health_url_file,
            #[cfg(test)]
            isolate_probe_environment: false,
        })
    }

    pub fn command_line_arguments(&self) -> Vec<String> {
        let mut arguments = vec![
            "run".into(),
            "--control-plane.api-key".into(),
            API_KEY_REFERENCE.into(),
            "--control-plane.base-url".into(),
            self.config.control_plane_base_url().into(),
            "--mcp.server-url".into(),
            self.config.mcp_target(),
            "--health.listen-addr".into(),
            "127.0.0.1:0".into(),
            "--health.url-file".into(),
            self.health_url_file.to_string_lossy().into_owned(),
            "--log.format".into(),
            "struct-text".into(),
            "--log.level".into(),
            "warn".into(),
        ];
        if self.mcp_guard_bearer.is_some() {
            for flag in ["--mcp.extra-headers", "--mcp.discovery-extra-headers"] {
                arguments.push(flag.into());
                arguments.push(format!("Authorization: env:{MCP_GUARD_BEARER_ENV}"));
            }
        }
        #[cfg(test)]
        let arguments = if self.config.embedded_mcp_stub() {
            let mut test_arguments = arguments;
            let mcp_index = test_arguments
                .iter()
                .position(|argument| argument == "--mcp.server-url")
                .expect("production argument set contains MCP Guard target");
            test_arguments.drain(mcp_index..=(mcp_index + 1));
            test_arguments.insert(1, "--embedded-mcp-stub".into());
            test_arguments
        } else {
            arguments
        };
        arguments
    }

    pub(crate) fn with_mcp_guard_bearer(mut self, bearer: SecretString) -> Self {
        self.mcp_guard_bearer = Some(bearer);
        self
    }

    pub fn health_url_file(&self) -> &Path {
        &self.health_url_file
    }

    pub fn spawn(self) -> Result<TunnelRuntime, TunnelError> {
        validate_api_key_reference(API_KEY_REFERENCE)?;
        let mut spec = ManagedProcessSpec::new("tunnel-client", &self.bundle.executable)
            .map_err(classify_supervisor)?
            .args(self.command_line_arguments())
            .current_dir(
                self.bundle
                    .executable
                    .parent()
                    .ok_or(TunnelError::RuntimeMissing)?,
            );
        for key in REMOVED_PARENT_ENV {
            spec = spec.env_remove(key).map_err(classify_supervisor)?;
        }
        #[cfg(test)]
        if self.isolate_probe_environment {
            for key in [
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "http_proxy",
                "https_proxy",
                "all_proxy",
            ] {
                spec = spec.env_remove(key).map_err(classify_supervisor)?;
            }
            spec = spec
                .env("NO_PROXY", "127.0.0.1,localhost")
                .map_err(classify_supervisor)?;
        }
        spec = spec
            .env(API_KEY_ENV, self.secret.expose_secret())
            .map_err(classify_supervisor)?;
        if let Some(bearer) = self.mcp_guard_bearer.as_ref() {
            spec = spec
                .env(
                    MCP_GUARD_BEARER_ENV,
                    &format!("Bearer {}", bearer.expose_secret()),
                )
                .map_err(classify_supervisor)?;
        }
        spec = spec
            .env(TUNNEL_ID_ENV, self.config.tunnel_id.expose())
            .map_err(classify_supervisor)?;
        spec = spec
            .env(
                "CONTROL_PLANE_BASE_URL",
                self.config.control_plane_base_url(),
            )
            .map_err(classify_supervisor)?;
        spec = spec
            .env("HEALTH_LISTEN_ADDR", "127.0.0.1:0")
            .map_err(classify_supervisor)?;
        spec = spec.env("DO_NOT_TRACK", "1").map_err(classify_supervisor)?;
        let supervisor = WindowsProcessSupervisor::spawn(&spec).map_err(classify_supervisor)?;
        Ok(TunnelRuntime {
            config: self.config,
            supervisor,
            health_url_file: self.health_url_file,
            health: None,
            connector_endpoint: None,
        })
    }
}

pub struct TunnelRuntime {
    config: TunnelRuntimeConfig,
    supervisor: WindowsProcessSupervisor,
    health_url_file: PathBuf,
    health: Option<HealthEndpoint>,
    connector_endpoint: Option<ConnectorEndpoint>,
}

impl fmt::Debug for TunnelRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TunnelRuntime")
            .field("config", &self.config)
            .field("process", self.supervisor.snapshot())
            .field("health_url_file", &self.health_url_file)
            .field("health_resolved", &self.health.is_some())
            .field(
                "connector_endpoint_available",
                &self.connector_endpoint.is_some(),
            )
            .finish()
    }
}

impl TunnelRuntime {
    pub fn start<C: CredentialStore>(
        config: TunnelRuntimeConfig,
        store: &C,
        timeout: Duration,
    ) -> Result<Self, TunnelError> {
        let mut runtime = PreparedTunnelStart::prepare(config, store)?.spawn()?;
        if let Err(error) = runtime.wait_ready(timeout) {
            let _ = runtime.stop();
            return Err(error);
        }
        Ok(runtime)
    }

    pub fn wait_ready(&mut self, timeout: Duration) -> Result<(), TunnelError> {
        let deadline = Instant::now() + timeout;
        loop {
            if !self
                .supervisor
                .root_is_running()
                .map_err(classify_supervisor)?
            {
                return Err(TunnelError::TunnelExited);
            }
            if self.health.is_none() {
                match fs::read_to_string(&self.health_url_file) {
                    Ok(value) => self.health = Some(HealthEndpoint::parse(&value)?),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err(TunnelError::HealthStateIo),
                }
            }
            if let Some(health) = self.health {
                match health.probe_ready_metadata() {
                    Ok(probe) if probe.ready => {
                        self.connector_endpoint = probe.connector_endpoint;
                        return Ok(());
                    }
                    Ok(_) | Err(TunnelError::HealthUnavailable) => {}
                    Err(error) => return Err(error),
                }
            }
            if Instant::now() >= deadline {
                return Err(TunnelError::HealthTimeout);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn wait_ready_for_recovery(
        &mut self,
        timeout: Duration,
        probe_timeout: Duration,
        cancelled: impl Fn() -> bool,
    ) -> Result<(), TunnelError> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancelled() {
                return Err(TunnelError::HealthUnavailable);
            }
            if !self
                .supervisor
                .root_is_running()
                .map_err(classify_supervisor)?
            {
                return Err(TunnelError::TunnelExited);
            }
            if self.health.is_none() {
                match fs::read_to_string(&self.health_url_file) {
                    Ok(value) => self.health = Some(HealthEndpoint::parse(&value)?),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err(TunnelError::HealthStateIo),
                }
            }
            if cancelled() {
                return Err(TunnelError::HealthUnavailable);
            }
            if let Some(health) = self.health {
                match health.probe_ready_metadata_with_timeout(probe_timeout) {
                    Ok(probe) if probe.ready => {
                        if cancelled() {
                            return Err(TunnelError::HealthUnavailable);
                        }
                        self.connector_endpoint = probe.connector_endpoint;
                        return Ok(());
                    }
                    Ok(_) | Err(TunnelError::HealthUnavailable) => {}
                    Err(error) => return Err(error),
                }
            }
            if cancelled() {
                return Err(TunnelError::HealthUnavailable);
            }
            if Instant::now() >= deadline {
                return Err(TunnelError::HealthTimeout);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub const fn process_snapshot(&self) -> &ProcessSnapshot {
        self.supervisor.snapshot()
    }
    pub fn root_is_running(&self) -> Result<bool, TunnelError> {
        self.supervisor
            .root_is_running()
            .map_err(classify_supervisor)
    }
    pub fn active_processes(&self) -> Result<u32, TunnelError> {
        self.supervisor
            .active_processes()
            .map_err(classify_supervisor)
    }
    pub fn stop(&mut self) -> Result<StopDisposition, TunnelError> {
        self.supervisor.force_stop().map_err(classify_supervisor)
    }
    pub fn config(&self) -> &TunnelRuntimeConfig {
        &self.config
    }
    pub fn connector_endpoint(&self) -> Option<ConnectorEndpoint> {
        self.connector_endpoint.clone()
    }
}

pub struct TunnelRestartPrimitive;

impl TunnelRestartPrimitive {
    pub fn prepare<C: CredentialStore>(
        config: TunnelRuntimeConfig,
        store: &C,
        fault: &TunnelError,
    ) -> Result<PreparedTunnelStart, TunnelError> {
        if fault.retryability() != Retryability::Recoverable {
            return Err(TunnelError::RestartDenied);
        }
        PreparedTunnelStart::prepare(config, store)
    }
}

fn validate_api_key_reference(reference: &str) -> Result<(), TunnelError> {
    if reference == API_KEY_REFERENCE && reference.strip_prefix("env:") == Some(API_KEY_ENV) {
        Ok(())
    } else {
        Err(TunnelError::SecretInjectionUnsupported)
    }
}

fn classify_supervisor(error: SupervisorError) -> TunnelError {
    match error {
        SupervisorError::WindowsApi {
            operation: "CreateProcessW",
            ..
        } => TunnelError::TunnelSpawnFailed(error),
        _ => TunnelError::ProcessOwnershipFailed(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{CredentialMetadata, CredentialStoreError};
    use crate::tunnel::{ControlPlaneFault, TunnelId};
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpListener};
    use std::process::Command;
    use std::sync::mpsc::{self, Sender};
    use std::thread::{self, JoinHandle};
    use std::time::{SystemTime, UNIX_EPOCH};

    const TUNNEL_ID: &str = "tunnel_0123456789abcdef0123456789abcdef";
    const SECRET_ONE: &str = "LB008_SYNTHETIC_RUNTIME_KEY_ONE_DO_NOT_LEAK";
    const SECRET_TWO: &str = "LB008_SYNTHETIC_RUNTIME_KEY_TWO_DO_NOT_LEAK";

    struct FakeStore {
        reads: Cell<usize>,
        values: RefCell<VecDeque<Option<String>>>,
    }

    impl FakeStore {
        fn new(values: impl IntoIterator<Item = Option<&'static str>>) -> Self {
            Self {
                reads: Cell::new(0),
                values: RefCell::new(
                    values
                        .into_iter()
                        .map(|value| value.map(str::to_owned))
                        .collect(),
                ),
            }
        }

        fn reads(&self) -> usize {
            self.reads.get()
        }
    }

    impl CredentialStore for FakeStore {
        fn save_runtime_api_key(
            &self,
            _secret: &SecretString,
        ) -> Result<CredentialMetadata, CredentialStoreError> {
            unreachable!("LB-008 fake store is read-only")
        }

        fn read_runtime_api_key(&self) -> Result<Option<SecretString>, CredentialStoreError> {
            self.reads.set(self.reads.get() + 1);
            self.values
                .borrow_mut()
                .pop_front()
                .unwrap_or(None)
                .map(SecretString::new)
                .transpose()
        }

        fn delete_runtime_api_key(&self) -> Result<bool, CredentialStoreError> {
            unreachable!("LB-008 fake store is read-only")
        }

        fn runtime_api_key_metadata(&self) -> Result<CredentialMetadata, CredentialStoreError> {
            unreachable!("LB-008 fake store is read-only")
        }
    }

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri has repository parent")
            .to_path_buf()
    }

    fn temp_health_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "localbridge-lb008-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn config(label: &str) -> TunnelRuntimeConfig {
        config_with_control_plane(label, "http://127.0.0.1:9")
    }

    fn config_with_control_plane(label: &str, base_url: &str) -> TunnelRuntimeConfig {
        let config = TunnelRuntimeConfig::new(
            repo_root(),
            temp_health_dir(label),
            TunnelId::new(TUNNEL_ID).unwrap(),
            65534,
        )
        .unwrap();
        #[cfg(debug_assertions)]
        {
            return config
                .with_test_control_plane_base_url(base_url)
                .unwrap()
                .with_test_embedded_mcp_stub();
        }
        #[allow(unreachable_code)]
        config
    }

    fn os_command_line(pid: u32) -> String {
        let script = format!(
            "$p=Get-CimInstance Win32_Process -Filter \"ProcessId = {pid}\"; if($p){{[Console]::Out.Write($p.CommandLine)}}"
        );
        for _ in 0..20 {
            let output = Command::new("powershell.exe")
                .args(["-NoProfile", "-Command", &script])
                .output()
                .expect("query Win32 process command line");
            if output.status.success() {
                let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                if !value.is_empty() {
                    return value;
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("tunnel-client command line was not observable");
    }

    fn blocked_control_plane() -> (String, Sender<()>, JoinHandle<()>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut request = [0u8; 4096];
                        let count = stream.read(&mut request).unwrap_or(0);
                        let request = String::from_utf8_lossy(&request[..count]);
                        if !request.starts_with("GET /v1/tunnels/") {
                            let body = r#"{"error":"not found"}"#;
                            let response = format!(
                                "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                body.len(),
                                body
                            );
                            let _ = stream.write_all(response.as_bytes());
                            continue;
                        }
                        let _ = release_rx.recv_timeout(Duration::from_secs(10));
                        let body = r#"{"error":"synthetic blocked control plane"}"#;
                        let response = format!(
                            "HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes());
                        return;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            return;
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("fake control plane accept failed: {error}"),
                }
            }
        });
        (format!("http://127.0.0.1:{port}"), release_tx, handle)
    }

    fn request_has_authorization(request: &str, expected_value: &str) -> bool {
        request
            .lines()
            .filter_map(|line| line.split_once(':'))
            .any(|(name, value)| {
                name.eq_ignore_ascii_case("authorization") && value.trim() == expected_value
            })
    }

    #[test]
    fn authorization_header_matching_follows_http_field_name_rules() {
        assert!(request_has_authorization(
            "GET / HTTP/1.1\r\nauthorization: Bearer expected\r\n\r\n",
            "Bearer expected"
        ));
        assert!(!request_has_authorization(
            "GET / HTTP/1.1\r\nAuthorization: Bearer wrong\r\n\r\n",
            "Bearer expected"
        ));
    }

    #[test]
    fn prepare_is_secret_redacted_and_unsupported_injection_fails_closed() {
        let store = FakeStore::new([Some(SECRET_ONE)]);
        let prepared = PreparedTunnelStart::prepare(config("prepare"), &store).unwrap();
        assert_eq!(store.reads(), 1);
        let rendered = prepared.command_line_arguments().join(" ");
        assert!(rendered.contains(API_KEY_REFERENCE));
        assert!(!rendered.contains(SECRET_ONE));
        assert!(!rendered.contains(TUNNEL_ID));
        assert!(!rendered.contains("--cloudflared.managed"));
        assert!(!rendered.contains("--cloudflared.path"));
        assert!(!rendered.contains("--cloudflared.token"));
        let debug = format!("{prepared:?}");
        assert!(!debug.contains(SECRET_ONE));
        assert!(matches!(
            validate_api_key_reference("literal-secret"),
            Err(TunnelError::SecretInjectionUnsupported)
        ));
        assert!(matches!(
            validate_api_key_reference("file:C:/plaintext-secret.txt"),
            Err(TunnelError::SecretInjectionUnsupported)
        ));
        let health_dir = prepared.config.health_state_dir.clone();
        drop(prepared);
        eprintln!("TEST_WORKSPACE_RETAINED path={}", health_dir.display());
    }

    #[test]
    fn inherited_runtime_override_isolation_covers_security_critical_surfaces() {
        for required in [
            "HEALTH_UNIX_SOCKET",
            "HEALTH_URL_FILE",
            "MCP_SERVER_URL",
            "MCP_COMMAND",
            "MCP_HTTP_PROXY",
            "HARPOON_ADDITIONAL_TRANSPORTS",
            "HARPOON_ALLOW_PLAINTEXT_HTTP",
            "HARPOON_TARGETS",
            "CONTROL_PLANE_EXTRA_HEADERS",
            "CONTROL_PLANE_POLL_CHANNELS",
            "CLOUDFLARED_TUNNEL_TOKEN",
            "TUNNEL_CLIENT_CONFIG",
            "TUNNEL_CLIENT_PROFILE",
            "LOG_HTTP_RAW_UNSAFE",
        ] {
            assert!(
                REMOVED_PARENT_ENV.contains(&required),
                "missing runtime env isolation: {required}"
            );
        }
        let mut names = REMOVED_PARENT_ENV.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            REMOVED_PARENT_ENV.len(),
            "duplicate runtime env isolation entry"
        );
        assert!(!REMOVED_PARENT_ENV.contains(&"HTTP_PROXY"));
        assert!(!REMOVED_PARENT_ENV.contains(&"HTTPS_PROXY"));
    }

    #[test]
    fn restart_primitive_refreshes_secret_only_for_recoverable_fault() {
        let store = FakeStore::new([Some(SECRET_ONE), Some(SECRET_TWO)]);
        let base = config("restart");
        let first = PreparedTunnelStart::prepare(base.clone(), &store).unwrap();
        assert_eq!(store.reads(), 1);
        drop(first);

        let second =
            TunnelRestartPrimitive::prepare(base.clone(), &store, &TunnelError::TunnelExited)
                .unwrap();
        assert_eq!(store.reads(), 2);
        assert!(!format!("{second:?}").contains(SECRET_TWO));
        drop(second);

        assert!(matches!(
            TunnelRestartPrimitive::prepare(
                base.clone(),
                &store,
                &TunnelError::ControlPlane(ControlPlaneFault::Authentication),
            ),
            Err(TunnelError::RestartDenied)
        ));
        assert_eq!(store.reads(), 2);
        eprintln!(
            "TEST_WORKSPACE_RETAINED path={}",
            base.health_state_dir.display()
        );
    }

    #[test]
    fn credential_and_configuration_faults_never_enter_restart_or_reread_secret() {
        let store = FakeStore::new([Some(SECRET_ONE)]);
        let base = config("non-recoverable");
        for fault in [
            TunnelError::RuntimeKeyMissing,
            TunnelError::InvalidTunnelId,
            TunnelError::InvalidMcpTarget,
        ] {
            assert_eq!(fault.retryability(), Retryability::NonRecoverable);
            assert!(matches!(
                TunnelRestartPrimitive::prepare(base.clone(), &store, &fault),
                Err(TunnelError::RestartDenied)
            ));
        }
        assert_eq!(
            store.reads(),
            0,
            "non-recoverable credential/configuration faults must not reread the secret"
        );
        if base.health_state_dir.exists() {
            eprintln!(
                "TEST_WORKSPACE_RETAINED path={}",
                base.health_state_dir.display()
            );
        }
    }

    // Only redacted facts cross the probe channel, never raw requests or credentials.
    #[derive(Debug)]
    struct ProbeObservation {
        method: String,
        path: String,
        has_authorization: bool,
        matches_authorization: bool,
        initialized: bool,
    }

    fn observe_probe_request(request: &str, initialized: bool) -> ProbeObservation {
        let mut start = request
            .lines()
            .next()
            .unwrap_or_default()
            .split_whitespace();
        ProbeObservation {
            method: start.next().unwrap_or_default().to_owned(),
            path: start.next().unwrap_or_default().to_owned(),
            has_authorization: request
                .lines()
                .filter_map(|line| line.split_once(':'))
                .any(|(name, _)| name.eq_ignore_ascii_case("authorization")),
            matches_authorization: request_has_authorization(
                request,
                &format!("Bearer {SECRET_TWO}"),
            ),
            initialized,
        }
    }

    // Startup traffic from the real client is concurrent: the oauth module's
    // WWW-Authenticate probe (POST then GET on the MCP URL, 1s deadline), the
    // RFC 9728 discovery GETs and the mcpclient initialize POST (2s deadline)
    // share one keep-alive HTTP client (tunnel-client pkg/oauth module.go and
    // pkg/mcpclient fxmodule.go). Serving connections serially stalls past
    // those deadlines and the client abandons the initialize, so every
    // connection is handled on its own thread. A real server also never
    // answers a connection that has not produced a request: the Go transport
    // can leave a dialed connection idle, and an unsolicited response here
    // races the client writing its first request on that connection, which
    // silently swallows the MCP initialize (connectStartupProbe wraps that
    // transport failure as ErrRejected and the client never retries it).
    // Connections without a request are therefore waited on or closed
    // silently, exactly like an idle keep-alive connection. Like the
    // production Guard, each connection then stays alive and serves further
    // requests: closing after one response leaves a window where the client
    // writes its next request onto the closing connection, and that swallowed
    // initialize is never retried either.
    fn serve_probe_connection(
        mut stream: std::net::TcpStream,
        request_tx: mpsc::Sender<ProbeObservation>,
    ) {
        let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
        let idle_deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let mut bytes = Vec::new();
            let mut byte = [0_u8; 1];
            while bytes.len() < 32_768 && !bytes.ends_with(b"\r\n\r\n") {
                match stream.read(&mut byte) {
                    Ok(0) => return, // peer closed: close silently
                    Ok(_) => bytes.push(byte[0]),
                    Err(_) if Instant::now() < idle_deadline => {} // idle: keep waiting
                    Err(_) => return,                              // idle past deadline: close
                }
            }
            let headers = String::from_utf8_lossy(&bytes);
            let length = headers
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if length > 65_536 {
                return;
            }
            let mut body = vec![0_u8; length];
            let mut body_read = 0;
            while body_read < length {
                match stream.read(&mut body[body_read..]) {
                    Ok(0) => return,
                    Ok(count) => body_read += count,
                    Err(_) if Instant::now() < idle_deadline => {}
                    Err(_) => return,
                }
            }
            let message: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let initialize = message["method"] == "initialize";
            let observation = observe_probe_request(&headers, initialize);
            let session = headers.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("mcp-session-id")
                    .then(|| value.trim().to_owned())
            });
            // Mirror the production Guard contract (mcp/server.rs): a sessionless
            // GET /mcp is rejected with 400 session_id_required, initialize and
            // tools/list responses carry Mcp-Session-Id, and the negotiated
            // protocol version is echoed from the client request.
            let (status, content_type, response, session_id) = if observation.path != "/mcp" {
                (
                    "404 Not Found",
                    "application/json",
                    serde_json::json!({"error":{"error_code":"InvalidRequest","phase":"mcp","cause":"endpoint_not_found","http_status":404}}).to_string(),
                    None,
                )
            } else if !observation.matches_authorization {
                (
                    "401 Unauthorized",
                    "application/json",
                    serde_json::json!({"error":{"error_code":"InvalidRequest","phase":"mcp","cause":"client_authentication_required","http_status":401}}).to_string(),
                    None,
                )
            } else if observation.method == "GET" {
                match session {
                    Some(_) => ("204 No Content", "", String::new(), None),
                    None => (
                        "400 Bad Request",
                        "application/json",
                        serde_json::json!({"error":{"error_code":"InvalidRequest","phase":"mcp","cause":"session_id_required","http_status":400}}).to_string(),
                        None,
                    ),
                }
            } else if initialize {
                (
                    "200 OK",
                    "application/json",
                    serde_json::json!({"jsonrpc":"2.0", "id":message["id"], "result":{
                        "protocolVersion":message["params"]["protocolVersion"].as_str().unwrap_or("2025-11-25"),
                        "capabilities":{"tools":{}},
                        "serverInfo":{"name":"localbridge-ci-probe", "version":"1"}
                    }})
                    .to_string(),
                    Some("localbridge-probe-session"),
                )
            } else if message["method"] == "tools/list" {
                (
                    "200 OK",
                    "application/json",
                    serde_json::json!({"jsonrpc":"2.0", "id":message["id"], "result":{"tools":[]}})
                        .to_string(),
                    Some("localbridge-probe-session"),
                )
            } else {
                ("202 Accepted", "", String::new(), None)
            };
            let mut head = format!("HTTP/1.1 {status}\r\n");
            if !content_type.is_empty() {
                head.push_str(&format!("Content-Type: {content_type}\r\n"));
            }
            if let Some(id) = session_id {
                head.push_str(&format!("Mcp-Session-Id: {id}\r\n"));
            }
            head.push_str(&format!("Content-Length: {}\r\n\r\n", response.len()));
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(response.as_bytes());
            let _ = request_tx.send(observation);
        }
    }

    #[test]
    fn probe_observations_distinguish_routes_and_redact_authorization() {
        for token in [None, Some("wrong"), Some(SECRET_TWO)] {
            let header = token
                .map(|value| format!("Authorization: Bearer {value}\r\n"))
                .unwrap_or_default();
            for path in ["/.well-known/oauth-protected-resource", "/mcp"] {
                let observation =
                    observe_probe_request(&format!("POST {path} HTTP/1.1\r\n{header}\r\n"), false);
                assert_eq!(observation.path, path);
                assert_eq!(observation.matches_authorization, token == Some(SECRET_TWO));
                assert_eq!(observation.has_authorization, token.is_some());
                assert!(!format!("{observation:?}").contains(SECRET_TWO));
            }
        }
    }

    #[test]
    fn actual_tunnel_discovery_sends_the_authenticated_pep_header() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (request_tx, request_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let probe = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(60);
            while Instant::now() < deadline && stop_rx.try_recv().is_err() {
                let Ok((stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                };
                let connection_tx = request_tx.clone();
                thread::spawn(move || serve_probe_connection(stream, connection_tx));
            }
        });
        let config = TunnelRuntimeConfig::new(
            repo_root(),
            temp_health_dir("authenticated-probe"),
            TunnelId::new(TUNNEL_ID).unwrap(),
            port,
        )
        .unwrap()
        .with_test_control_plane_base_url("http://127.0.0.1:9")
        .unwrap();
        let prepared = PreparedTunnelStart::prepare(config, &FakeStore::new([Some(SECRET_ONE)]));
        let mut observations = Vec::new();
        let mut failure = None;
        let mut initialized = false;
        match prepared {
            Ok(mut prepared) => {
                prepared.mcp_guard_bearer = Some(SecretString::new(SECRET_TWO).unwrap());
                prepared.isolate_probe_environment = true;
                match prepared.spawn() {
                    Ok(mut runtime) => {
                        // Runner variance (cold cache, real-time scanning) delays
                        // client startup far more than the assertions below; the
                        // window only bounds how long the probe collects facts.
                        let deadline = Instant::now() + Duration::from_secs(30);
                        while Instant::now() < deadline {
                            match request_rx.recv_timeout(Duration::from_millis(100)) {
                                Ok(observation) => {
                                    let bad = observation.path == "/mcp"
                                        && !observation.matches_authorization;
                                    initialized |= observation.path == "/mcp"
                                        && observation.initialized
                                        && observation.matches_authorization;
                                    observations.push(observation);
                                    if bad {
                                        failure =
                                            Some("MCP request authentication mismatch".to_string());
                                        break;
                                    }
                                    if initialized
                                        && observations
                                            .iter()
                                            .any(|item| item.path == "/mcp" && !item.initialized)
                                    {
                                        break;
                                    }
                                }
                                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                                Err(mpsc::RecvTimeoutError::Timeout) => {}
                            }
                            match runtime.supervisor.root_is_running() {
                                Ok(true) => {}
                                state => {
                                    failure = Some(format!(
                                        "Tunnel exited before probe completed: {state:?}"
                                    ));
                                    break;
                                }
                            }
                        }
                        let process = runtime.supervisor.snapshot();
                        eprintln!("Tunnel probe process: {process:?}");
                        if let Err(error) = runtime.stop() {
                            failure = Some(format!("Tunnel stop failed: {error:?}"));
                        }
                    }
                    Err(error) => failure = Some(format!("Tunnel spawn failed: {error:?}")),
                }
            }
            Err(error) => failure = Some(format!("Tunnel prepare failed: {error:?}")),
        }
        let _ = stop_tx.send(());
        let joined = probe.join();
        observations.extend(request_rx.try_iter());
        assert!(joined.is_ok(), "probe server failed");
        assert!(
            failure.is_none(),
            "{failure:?}; observations={observations:?}"
        );
        assert!(
            initialized,
            "no authenticated MCP initialization before deadline; observations={observations:?}"
        );
        assert!(
            observations
                .iter()
                .filter(|item| item.path == "/mcp")
                .all(|item| item.matches_authorization),
            "MCP authentication mismatch; observations={observations:?}"
        );
        assert!(
            observations
                .iter()
                .any(|item| item.path == "/mcp" && item.method == "POST"),
            "no MCP POST observed"
        );
    }

    #[test]
    fn actual_process_command_line_never_contains_runtime_secret_and_job_stop_drains() {
        let (control_plane, release_control_plane, control_plane_thread) = blocked_control_plane();
        let store = FakeStore::new([Some(SECRET_ONE)]);
        let prepared = PreparedTunnelStart::prepare(
            config_with_control_plane("process", &control_plane),
            &store,
        )
        .unwrap();
        let health_dir = prepared.config.health_state_dir.clone();
        let mut runtime = prepared
            .spawn()
            .expect("vendored tunnel-client must spawn locally");
        assert!(runtime.root_is_running().unwrap());
        assert!(runtime.active_processes().unwrap() >= 1);

        let command_line = os_command_line(runtime.process_snapshot().pid);
        assert!(command_line.contains("tunnel-client.exe"));
        assert!(command_line.contains(API_KEY_REFERENCE));
        assert!(!command_line.contains(SECRET_ONE));
        assert!(!command_line.contains(TUNNEL_ID));
        assert!(!command_line.contains("--cloudflared.managed"));
        assert!(!command_line.contains("--cloudflared.path"));
        assert!(!command_line.contains("--cloudflared.token"));
        assert!(!format!("{runtime:?}").contains(SECRET_ONE));

        let _ = release_control_plane.send(());
        runtime.stop().expect("Job-owned tunnel stop");
        assert!(!runtime.root_is_running().unwrap());
        assert_eq!(runtime.active_processes().unwrap(), 0);
        drop(runtime);
        control_plane_thread.join().unwrap();
        eprintln!("TEST_WORKSPACE_RETAINED path={}", health_dir.display());
    }
}
