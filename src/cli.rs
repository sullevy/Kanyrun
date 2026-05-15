use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliRequest {
    pub force_menu: bool,
    #[serde(default)]
    pub root_menu: bool,
    pub test_menu: bool,
    pub text: Option<String>,
    pub files: Vec<std::path::PathBuf>,
    pub menu_file: Option<std::path::PathBuf>,
    pub from_clipboard: bool,
    pub from_primary: bool,
    pub oneshot: bool,
    pub daemon_server: bool,
    #[serde(default)]
    pub debug: bool,
    pub daemon_idle_timeout_secs: Option<u64>,
    pub ui_idle_timeout_secs: Option<u64>,
}

#[derive(Debug)]
pub enum ParseOutcome {
    Request(CliRequest),
    Help(String),
}

pub fn parse() -> Result<ParseOutcome, String> {
    parse_from(std::env::args())
}

pub fn parse_from<I, S>(args: I) -> Result<ParseOutcome, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut request = CliRequest::default();
    let mut args = args.into_iter().map(Into::into).peekable();

    let program = args.next().unwrap_or_else(|| "kanyrun".into());

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(ParseOutcome::Help(help_text(&program))),
            "--menu" => request.force_menu = true,
            "--root-menu" => request.root_menu = true,
            "--test" => request.test_menu = true,
            "--menu-file" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --menu-file".to_string())?;
                request.menu_file = Some(std::path::PathBuf::from(value));
            }
            "--text" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --text".to_string())?;
                request.text = Some(value);
            }
            "--files" => {
                let mut found = false;

                while let Some(next) = args.peek() {
                    if next.starts_with("--") {
                        break;
                    }

                    found = true;
                    let path = args.next().expect("peeked value must exist");
                    request.files.push(std::path::PathBuf::from(path));
                }

                if !found {
                    return Err("missing value for --files".into());
                }
            }
            "--from-clipboard" => request.from_clipboard = true,
            "--from-primary" => request.from_primary = true,
            "--oneshot" => request.oneshot = true,
            "--daemon-server" => request.daemon_server = true,
            "--debug" => request.debug = true,
            "--daemon-idle-timeout" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --daemon-idle-timeout".to_string())?;
                request.daemon_idle_timeout_secs =
                    Some(parse_timeout_secs("--daemon-idle-timeout", &value)?);
            }
            "--ui-idle-timeout" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --ui-idle-timeout".to_string())?;
                request.ui_idle_timeout_secs =
                    Some(parse_timeout_secs("--ui-idle-timeout", &value)?);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(ParseOutcome::Request(request))
}

fn help_text(program: &str) -> String {
    format!(
        "Usage: {program} [OPTIONS]\n\n
actions:\n  no explicit input        Show the default root menu\n  --root-menu              Show the default root menu without reading selection or clipboard\n  --text VALUE             Use the provided text or URL as context\n  --files PATH...          Use one or more selected file or directory paths\n\noptions:\n  --menu                   Force a menu even when a direct action rule matches\n  --test                   Force the first configured menu to display\n  --menu-file PATH         Load menus from a specific menu.ini or menu.conf file\n  --from-clipboard         Use clipboard content as context\n  --from-primary           Use primary selection as context\n  --oneshot                Bypass the resident daemon and run once in the current process\n  --daemon-idle-timeout S  Exit the daemon after S seconds of inactivity\n  --ui-idle-timeout S      Reap the warm UI child after S seconds of inactivity\n  --debug                  Enable timing logs for launch diagnostics\n  -h, --help               Show this help message\n\nexamples:\n  {program}\n  {program} --root-menu\n  {program} --test\n  {program} --menu-file ~/.config/kanyrun/menu-work.ini --test\n  {program} --text 'https://example.com'\n  {program} --text 'hello world' --menu\n  {program} --from-primary --menu\n  {program} --from-clipboard --menu\n  {program} --files /tmp/file.pdf\n  {program} --files /tmp/a.txt /tmp/b.txt\n  {program} --daemon-idle-timeout 300 --ui-idle-timeout 15\n"
    )
}

fn parse_timeout_secs(flag: &str, value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{flag} expects an integer number of seconds"))
}

#[cfg(test)]
mod tests {
    use super::{CliRequest, ParseOutcome, parse_from};
    use std::path::PathBuf;

    #[test]
    fn parses_text_and_menu_flags() {
        let outcome = parse_from(["kanyrun", "--menu", "--text", "hello world"]).unwrap();

        assert!(matches!(
            outcome,
            ParseOutcome::Request(CliRequest {
                force_menu: true,
                root_menu: false,
                test_menu: false,
                text: Some(ref value),
                files,
                menu_file: None,
                from_clipboard: false,
                from_primary: false,
                oneshot: false,
                daemon_server: false,
                debug: false,
                daemon_idle_timeout_secs: None,
                ui_idle_timeout_secs: None,
            }) if value == "hello world" && files.is_empty()
        ));
    }

    #[test]
    fn parses_multiple_files() {
        let outcome = parse_from([
            "kanyrun",
            "--files",
            "/tmp/one.txt",
            "/tmp/two.pdf",
            "--from-primary",
        ])
        .unwrap();

        match outcome {
            ParseOutcome::Request(request) => {
                assert_eq!(
                    request.files,
                    vec![PathBuf::from("/tmp/one.txt"), PathBuf::from("/tmp/two.pdf")]
                );
                assert!(request.from_primary);
                assert!(!request.oneshot);
            }
            ParseOutcome::Help(_) => panic!("expected request"),
        }
    }

    #[test]
    fn parses_menu_file_override() {
        let outcome =
            parse_from(["kanyrun", "--menu-file", "/tmp/menu-work.ini", "--test"]).unwrap();

        assert!(matches!(
            outcome,
            ParseOutcome::Request(CliRequest {
                test_menu: true,
                menu_file: Some(ref path),
                ..
            }) if path == &PathBuf::from("/tmp/menu-work.ini")
        ));
    }

    #[test]
    fn parses_root_menu_flag() {
        let outcome = parse_from(["kanyrun", "--root-menu"]).unwrap();

        assert!(matches!(
            outcome,
            ParseOutcome::Request(CliRequest {
                root_menu: true,
                force_menu: false,
                text: None,
                files,
                ..
            }) if files.is_empty()
        ));
    }

    #[test]
    fn returns_help_output() {
        let outcome = parse_from(["kanyrun", "--help"]).unwrap();

        match outcome {
            ParseOutcome::Help(help) => {
                assert!(help.contains("Usage: kanyrun [OPTIONS]"));
                assert!(help.contains("--files PATH..."));
                assert!(help.contains("--test"));
                assert!(help.contains("--root-menu"));
                assert!(help.contains("--menu-file PATH"));
                assert!(help.contains("--debug"));
            }
            ParseOutcome::Request(_) => panic!("expected help"),
        }
    }

    #[test]
    fn parses_test_flag() {
        let outcome = parse_from(["kanyrun", "--test"]).unwrap();

        assert!(matches!(
            outcome,
            ParseOutcome::Request(CliRequest {
                test_menu: true,
                force_menu: false,
                root_menu: false,
                text: None,
                files,
                menu_file: None,
                from_clipboard: false,
                from_primary: false,
                oneshot: false,
                daemon_server: false,
                debug: false,
                daemon_idle_timeout_secs: None,
                ui_idle_timeout_secs: None,
            }) if files.is_empty()
        ));
    }

    #[test]
    fn parses_debug_flag() {
        let outcome = parse_from(["kanyrun", "--debug", "--menu"]).unwrap();

        assert!(matches!(
            outcome,
            ParseOutcome::Request(CliRequest {
                debug: true,
                force_menu: true,
                ..
            })
        ));
    }

    #[test]
    fn rejects_files_flag_without_values() {
        let error = parse_from(["kanyrun", "--files"]).unwrap_err();

        assert!(error.contains("--files"));
    }

    #[test]
    fn parses_daemon_timeouts_and_oneshot() {
        let outcome = parse_from([
            "kanyrun",
            "--oneshot",
            "--daemon-idle-timeout",
            "300",
            "--ui-idle-timeout",
            "15",
        ])
        .unwrap();

        match outcome {
            ParseOutcome::Request(request) => {
                assert!(request.oneshot);
                assert_eq!(request.daemon_idle_timeout_secs, Some(300));
                assert_eq!(request.ui_idle_timeout_secs, Some(15));
            }
            ParseOutcome::Help(_) => panic!("expected request"),
        }
    }
}
