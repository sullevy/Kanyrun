use crate::actions::SystemCommandRunner;
use crate::app::{WlPasteSelectionReader, run_with};
use crate::cli::CliRequest;
use crate::config::{ConfigPaths, LoadedConfig, config_paths, load};
use crate::ui::WarmPresenter;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

const DEFAULT_DAEMON_IDLE_TIMEOUT_SECS: u64 = 300;
const DEFAULT_UI_IDLE_TIMEOUT_SECS: u64 = 15;
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Invocation {
    request: CliRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InvocationResponse {
    error: Option<String>,
}

pub fn dispatch(mut request: CliRequest) -> Result<(), String> {
    request.daemon_server = false;
    request.oneshot = false;

    let payload = serde_json::to_vec(&Invocation { request })
        .map_err(|error| format!("failed to encode daemon request: {error}"))?;

    if let Err(error) = send_invocation(&socket_path_for_client(), &payload) {
        remove_unreachable_sockets();
        spawn_daemon()?;
        send_invocation(&socket_path_for_client(), &payload)
            .map_err(|retry_error| format!("{error}; then failed to contact daemon: {retry_error}"))?;
    }

    Ok(())
}

pub fn serve(request: &CliRequest) -> Result<(), String> {
    let (listener, socket_path) = bind_listener()?;
    let _guard = SocketGuard::new(socket_path.clone());
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("failed to mark daemon socket non-blocking: {error}"))?;

    let mut state = DaemonState::new(
        request
            .daemon_idle_timeout_secs
            .unwrap_or(DEFAULT_DAEMON_IDLE_TIMEOUT_SECS),
        request
            .ui_idle_timeout_secs
            .unwrap_or(DEFAULT_UI_IDLE_TIMEOUT_SECS),
    )?;
    let mut last_request_at = Instant::now();

    loop {
        state.presenter.stop_if_idle();
        if last_request_at.elapsed() >= state.daemon_idle_timeout {
            break;
        }

        match listener.accept() {
            Ok((mut stream, _)) => {
                handle_connection(&mut stream, &mut state)?;
                last_request_at = Instant::now();
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(error) => return Err(format!("failed to accept daemon connection: {error}")),
        }
    }

    state.presenter.shutdown();
    Ok(())
}

fn send_invocation(socket_path: &Path, payload: &[u8]) -> Result<(), String> {
    let mut stream = UnixStream::connect(socket_path)
        .map_err(|error| format!("failed to connect to daemon socket {}: {error}", socket_path.display()))?;
    stream
        .write_all(payload)
        .map_err(|error| format!("failed to send daemon request: {error}"))?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|error| format!("failed to finalize daemon request: {error}"))?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| format!("failed to read daemon response: {error}"))?;
    let response: InvocationResponse = serde_json::from_str(&response)
        .map_err(|error| format!("failed to decode daemon response: {error}"))?;

    match response.error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}


fn remove_unreachable_sockets() {
    for socket_path in socket_candidates() {
        if socket_path.exists() && UnixStream::connect(&socket_path).is_err() {
            let _ = fs::remove_file(socket_path);
        }
    }
}

fn spawn_daemon() -> Result<(), String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("failed to resolve current executable for daemon spawn: {error}"))?;
    Command::new(current_exe)
        .arg("--daemon-server")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("failed to spawn daemon: {error}"))?;

    for _ in 0..30 {
        if socket_candidates().iter().any(|path| path.exists()) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }

    Err("daemon did not become ready in time".into())
}

fn handle_connection(stream: &mut UnixStream, state: &mut DaemonState) -> Result<(), String> {
    let mut payload = Vec::new();
    stream
        .read_to_end(&mut payload)
        .map_err(|error| format!("failed to read daemon request: {error}"))?;
    let invocation: Invocation = serde_json::from_slice(&payload)
        .map_err(|error| format!("failed to decode daemon request: {error}"))?;

    if let Some(timeout) = invocation.request.daemon_idle_timeout_secs {
        state.daemon_idle_timeout = Duration::from_secs(timeout);
    }
    if let Some(timeout) = invocation.request.ui_idle_timeout_secs {
        state.presenter_idle_timeout = Duration::from_secs(timeout);
        state.presenter.shutdown();
        state.presenter = state.build_presenter()?;
    }

    let result = state.handle_request(&invocation.request);
    let response = InvocationResponse {
        error: result.err(),
    };
    let body = serde_json::to_vec(&response)
        .map_err(|error| format!("failed to encode daemon response: {error}"))?;
    stream
        .write_all(&body)
        .map_err(|error| format!("failed to write daemon response: {error}"))
}

fn bind_listener() -> Result<(UnixListener, PathBuf), String> {
    let mut errors = Vec::new();

    for socket_path in socket_candidates() {
        if socket_path.exists() {
            let _ = fs::remove_file(&socket_path);
        }

        if let Some(parent) = socket_path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            errors.push(format!(
                "failed to create daemon runtime directory {}: {error}",
                parent.display()
            ));
            continue;
        }

        match UnixListener::bind(&socket_path) {
            Ok(listener) => return Ok((listener, socket_path)),
            Err(error) => errors.push(format!(
                "failed to bind daemon socket {}: {error}",
                socket_path.display()
            )),
        }
    }

    Err(errors.join("; "))
}

fn socket_path_for_client() -> PathBuf {
    socket_candidates()
        .into_iter()
        .find(|path| path.exists())
        .unwrap_or_else(|| socket_candidates().into_iter().next().expect("at least one socket candidate"))
}

fn socket_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
        candidates.push(runtime_dir.join("kanyrun").join("daemon.sock"));
    }

    let cache_base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")));
    if let Some(cache_dir) = cache_base {
        candidates.push(cache_dir.join("kanyrun").join("daemon.sock"));
    }

    candidates.push(PathBuf::from("/tmp/kanyrun/daemon.sock"));
    candidates
}

struct SocketGuard {
    path: PathBuf,
}

impl SocketGuard {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

struct DaemonState {
    config_paths: ConfigPaths,
    config_cache: Option<CachedConfig>,
    daemon_idle_timeout: Duration,
    presenter_idle_timeout: Duration,
    presenter: WarmPresenter,
}

struct CachedConfig {
    loaded: LoadedConfig,
    config_modified_at: Option<SystemTime>,
    menu_modified_at: Option<SystemTime>,
}

impl DaemonState {
    fn new(daemon_idle_timeout_secs: u64, presenter_idle_timeout_secs: u64) -> Result<Self, String> {
        let config_paths = config_paths();
        let presenter_idle_timeout = Duration::from_secs(presenter_idle_timeout_secs);
        let loaded = load(&config_paths, None)?;
        let presenter = WarmPresenter::from_config(&loaded, presenter_idle_timeout)?;
        let cached = CachedConfig::from_loaded(loaded)?;

        Ok(Self {
            config_paths,
            config_cache: Some(cached),
            daemon_idle_timeout: Duration::from_secs(daemon_idle_timeout_secs),
            presenter_idle_timeout,
            presenter,
        })
    }

    fn build_presenter(&self) -> Result<WarmPresenter, String> {
        let config = self
            .config_cache
            .as_ref()
            .map(|cached| &cached.loaded)
            .ok_or_else(|| "daemon presenter requested before config was loaded".to_string())?;
        WarmPresenter::from_config(config, self.presenter_idle_timeout)
    }

    fn handle_request(&mut self, request: &CliRequest) -> Result<(), String> {
        let config = self.load_config_for_request(request)?;
        let reader = WlPasteSelectionReader;
        let mut runner = SystemCommandRunner;
        run_with(request, &config, &reader, &mut self.presenter, &mut runner)
    }

    fn load_config_for_request(&mut self, request: &CliRequest) -> Result<LoadedConfig, String> {
        if request.menu_file.is_some() {
            return load(&self.config_paths, request.menu_file.as_deref());
        }

        let should_reload = match self.config_cache.as_ref() {
            Some(cached) => {
                cached.config_modified_at != modified_time(&cached.loaded.sources.config_toml)
                    || cached.menu_modified_at != modified_time(&cached.loaded.sources.menu_file)
            }
            None => true,
        };

        if should_reload {
            let loaded = load(&self.config_paths, None)?;
            self.presenter.shutdown();
            self.config_cache = Some(CachedConfig::from_loaded(loaded.clone())?);
            self.presenter = WarmPresenter::from_config(&loaded, self.presenter_idle_timeout)?;
        }

        self.config_cache
            .as_ref()
            .map(|cached| cached.loaded.clone())
            .ok_or_else(|| "daemon config cache is empty".to_string())
    }
}

impl CachedConfig {
    fn from_loaded(loaded: LoadedConfig) -> Result<Self, String> {
        Ok(Self {
            config_modified_at: modified_time(&loaded.sources.config_toml),
            menu_modified_at: modified_time(&loaded.sources.menu_file),
            loaded,
        })
    }
}

fn modified_time(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}
#[cfg(test)]
mod tests {
    use super::{Invocation, InvocationResponse};
    use crate::cli::CliRequest;

    #[test]
    fn serializes_invocation_round_trip() {
        let invocation = Invocation {
            request: CliRequest {
                text: Some("hello".into()),
                daemon_idle_timeout_secs: Some(300),
                ui_idle_timeout_secs: Some(15),
                ..CliRequest::default()
            },
        };

        let encoded = serde_json::to_vec(&invocation).unwrap();
        let decoded: Invocation = serde_json::from_slice(&encoded).unwrap();

        assert_eq!(decoded.request.text.as_deref(), Some("hello"));
        assert_eq!(decoded.request.daemon_idle_timeout_secs, Some(300));
        assert_eq!(decoded.request.ui_idle_timeout_secs, Some(15));
    }

    #[test]
    fn serializes_response_round_trip() {
        let response = InvocationResponse {
            error: Some("boom".into()),
        };

        let encoded = serde_json::to_string(&response).unwrap();
        let decoded: InvocationResponse = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.error.as_deref(), Some("boom"));
    }
}
