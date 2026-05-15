use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn main() {
    if let Err(error) = run() {
        eprintln!("get-path: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let paths = if args.is_empty() {
        paths_from_dolphin()?
    } else {
        collect_paths(args.iter().map(String::as_str))
    };

    if paths.is_empty() {
        return Err("no valid paths found".to_string());
    }

    for path in paths {
        println!("{}", path.display());
    }

    Ok(())
}

fn collect_paths<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut paths = Vec::new();

    for value in values {
        if let Some(path) = normalize_candidate(value) {
            if seen.insert(path.clone()) {
                paths.push(path);
            }
        }
    }

    paths
}

fn normalize_candidate(value: &str) -> Option<PathBuf> {
    let trimmed = value
        .trim_end_matches('\r')
        .trim_matches('"')
        .trim_matches('\'');
    let path = if let Some(uri) = trimmed.strip_prefix("file://") {
        let without_host = uri.strip_prefix("localhost").unwrap_or(uri);
        decode_percent(without_host)
    } else {
        trimmed.to_string()
    };

    let path = Path::new(&path);
    if !path.exists() {
        return None;
    }

    fs::canonicalize(path).ok()
}

fn decode_percent(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                output.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&output).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn paths_from_dolphin() -> Result<Vec<PathBuf>, String> {
    ensure_command("busctl")?;
    ensure_command("wl-paste")?;
    ensure_command("wl-copy")?;

    let clipboard = ClipboardState::save();
    let candidates = dolphin_candidates()?;

    for candidate in candidates {
        let before_uri = wl_paste("text/uri-list");
        let before_plain = wl_paste("text/plain");
        let token = format!("get-path-{}-{}", std::process::id(), unix_nanos());
        wl_copy("text/plain", token.as_bytes())?;

        if trigger_dolphin_copy(&candidate) {
            if let Some(paths) = wait_for_fresh_paths(&token, &before_uri, &before_plain) {
                clipboard.restore();
                return Ok(paths);
            }
        }
    }

    clipboard.restore();
    Err("no fresh paths copied from Dolphin".to_string())
}

fn ensure_command(command: &str) -> Result<(), String> {
    Command::new(command)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|_| ())
        .map_err(|_| format!("{command} is required"))
}

#[derive(Clone)]
struct DolphinCandidate {
    service: String,
    window: String,
}

fn dolphin_candidates() -> Result<Vec<DolphinCandidate>, String> {
    let mut active = Vec::new();
    let mut inactive = Vec::new();

    for service in dolphin_services()? {
        for window in dolphin_windows(&service) {
            let candidate = DolphinCandidate {
                service: service.clone(),
                window,
            };
            if dolphin_window_is_active(&candidate) {
                active.push(candidate);
            } else {
                inactive.push(candidate);
            }
        }
    }

    active.extend(inactive);
    Ok(active)
}

fn dolphin_services() -> Result<Vec<String>, String> {
    let output = command_output(Command::new("busctl").args([
        "--user",
        "--no-pager",
        "--no-legend",
        "--acquired",
        "list",
    ]))?;
    Ok(output
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|service| {
            service.starts_with("org.kde.dolphin-")
                && service[16..].chars().all(|c| c.is_ascii_digit())
        })
        .map(str::to_string)
        .collect())
}

fn dolphin_windows(service: &str) -> Vec<String> {
    let output = command_output(Command::new("busctl").args([
        "--user",
        "--no-pager",
        "--list",
        "tree",
        service,
    ]));
    let Ok(output) = output else {
        return Vec::new();
    };

    output
        .lines()
        .filter(|line| {
            line.starts_with("/dolphin/Dolphin_") && line[17..].chars().all(|c| c.is_ascii_digit())
        })
        .map(str::to_string)
        .collect()
}

fn dolphin_window_is_active(candidate: &DolphinCandidate) -> bool {
    busctl_last_word(Command::new("busctl").args([
        "--user",
        "call",
        &candidate.service,
        &candidate.window,
        "org.kde.dolphin.MainWindow",
        "isActiveWindow",
    ]))
    .as_deref()
        == Some("true")
}

fn trigger_dolphin_copy(candidate: &DolphinCandidate) -> bool {
    let edit_copy = format!("{}/actions/edit_copy", candidate.window);
    let copy_location = format!("{}/actions/copy_location", candidate.window);

    action_enabled(&candidate.service, &edit_copy) && trigger_action(&candidate.service, &edit_copy)
        || action_enabled(&candidate.service, &copy_location)
            && trigger_action(&candidate.service, &copy_location)
}

fn action_enabled(service: &str, object_path: &str) -> bool {
    busctl_last_word(Command::new("busctl").args([
        "--user",
        "get-property",
        service,
        object_path,
        "org.qtproject.Qt.QAction",
        "enabled",
    ]))
    .as_deref()
        == Some("true")
}

fn trigger_action(service: &str, object_path: &str) -> bool {
    Command::new("busctl")
        .args([
            "--user",
            "call",
            service,
            object_path,
            "org.qtproject.Qt.QAction",
            "trigger",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn wait_for_fresh_paths(token: &str, before_uri: &str, before_plain: &str) -> Option<Vec<PathBuf>> {
    let attempts = env_usize("GET_PATH_WAIT_ATTEMPTS", 40);
    let delay = Duration::from_millis(env_usize("GET_PATH_WAIT_DELAY_MS", 50) as u64);

    for _ in 0..attempts {
        let raw_uri = wl_paste("text/uri-list");
        if !raw_uri.is_empty() && raw_uri != before_uri {
            let lines = raw_uri.lines().filter_map(|line| {
                let line = line.trim_end_matches('\r');
                if line.is_empty() || line.starts_with('#') || !line.starts_with("file://") {
                    None
                } else {
                    Some(line)
                }
            });
            let paths = collect_paths(lines);
            if !paths.is_empty() {
                return Some(paths);
            }
        }

        let raw_plain = wl_paste("text/plain");
        if !raw_plain.is_empty() && raw_plain != before_plain && raw_plain != token {
            let paths = collect_paths(raw_plain.lines());
            if !paths.is_empty() {
                return Some(paths);
            }
        }

        if !delay.is_zero() {
            thread::sleep(delay);
        }
    }

    None
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn wl_paste(mime_type: &str) -> String {
    command_output(Command::new("wl-paste").args(["--no-newline", "--type", mime_type]))
        .unwrap_or_default()
}

fn wl_copy(mime_type: &str, data: &[u8]) -> Result<(), String> {
    let mut child = Command::new("wl-copy")
        .args(["--type", mime_type])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "failed to run wl-copy".to_string())?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(data)
            .map_err(|_| "failed to write to wl-copy".to_string())?;
    }

    child
        .wait()
        .map_err(|_| "failed to wait for wl-copy".to_string())
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err("wl-copy failed".to_string())
            }
        })
}

struct ClipboardState {
    mime_type: Option<String>,
    content: Vec<u8>,
}

impl ClipboardState {
    fn save() -> Self {
        let types =
            command_output(Command::new("wl-paste").arg("--list-types")).unwrap_or_default();
        for mime_type in types.lines().filter(|line| !line.is_empty()) {
            if let Ok(content) = command_bytes(Command::new("wl-paste").args(["--type", mime_type]))
            {
                return Self {
                    mime_type: Some(mime_type.to_string()),
                    content,
                };
            }
        }

        Self {
            mime_type: None,
            content: Vec::new(),
        }
    }

    fn restore(&self) {
        if let Some(mime_type) = &self.mime_type {
            let _ = wl_copy(mime_type, &self.content);
        } else {
            let _ = Command::new("wl-copy")
                .arg("--clear")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

fn command_output(command: &mut Command) -> Result<String, String> {
    let output = command
        .output()
        .map_err(|_| "failed to run command".to_string())?;
    if !output.status.success() {
        return Err("command failed".to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn command_bytes(command: &mut Command) -> Result<Vec<u8>, String> {
    let output = command
        .output()
        .map_err(|_| "failed to run command".to_string())?;
    if !output.status.success() {
        return Err("command failed".to_string());
    }

    Ok(output.stdout)
}

fn busctl_last_word(command: &mut Command) -> Option<String> {
    command_output(command)
        .ok()?
        .split_whitespace()
        .last()
        .map(str::to_string)
}

fn unix_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}
