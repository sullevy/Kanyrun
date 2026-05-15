use crate::config::{LoadedConfig, render_quickmenu_command_template};
use crate::context::{Context, ContextKind};
use crate::menu::ActionCommand;
use std::path::PathBuf;
use std::process::Command;

pub trait CommandRunner {
    fn run(&mut self, program: &str, args: &[String]) -> Result<(), String>;
}

pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&mut self, program: &str, args: &[String]) -> Result<(), String> {
        let status = Command::new(program)
            .args(args)
            .status()
            .map_err(|error| format!("failed to spawn {program}: {error}"))?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("command {program} exited with status {status}"))
        }
    }
}

pub fn execute<R>(
    config: &LoadedConfig,
    context: &Context,
    command: &ActionCommand,
    runner: &mut R,
) -> Result<(), String>
where
    R: CommandRunner,
{
    match command {
        ActionCommand::OpenUrl => run_command(
            runner,
            browser_program(config),
            vec![context_url(context)?.to_string()],
        ),
        ActionCommand::Search { provider } => {
            let provider = config
                .app
                .providers
                .get(provider)
                .ok_or_else(|| format!("unknown provider: {provider}"))?;
            let query = context
                .text
                .as_deref()
                .ok_or_else(|| "search action requires text context".to_string())?;
            let url = provider.url.replace("{query}", &urlencoding::encode(query));
            run_command(runner, browser_program(config), vec![url])
        }
        ActionCommand::CopyText => run_template(
            runner,
            configured_copy_command(config),
            &[("{text}", context_text(context)?)],
        ),
        ActionCommand::CopyPath => run_template(
            runner,
            configured_copy_command(config),
            &[
                ("{text}", context_path(context)?),
                ("{path}", context_path(context)?),
            ],
        ),
        ActionCommand::QuickUrl { url } => run_command(
            runner,
            browser_program(config),
            vec![render_quickmenu_url(url, context)?],
        ),
        ActionCommand::QuickPath { path } => run_command(
            runner,
            "xdg-open",
            vec![render_quickmenu_path(path, context)?],
        ),
        ActionCommand::OpenFile => {
            run_command(runner, "xdg-open", vec![context_path(context)?.to_string()])
        }
        ActionCommand::OpenDirectory => run_command(
            runner,
            configured_open_directory_program(config),
            configured_open_directory_args(config, context_directory(context)?),
        ),
        ActionCommand::RevealFile => run_template(
            runner,
            configured_reveal_command(config),
            &[
                ("{path}", context_path(context)?),
                ("{directory}", context_directory(context)?),
                ("{text}", context_text(context)?),
            ],
        ),
        ActionCommand::OpenWith { desktop_id } => run_command(
            runner,
            "gtk-launch",
            vec![desktop_id.clone(), context_path(context)?.to_string()],
        ),
        ActionCommand::Shell { command } => run_shell_command(runner, command, context),
        ActionCommand::Separator => Ok(()),
    }
}

fn run_command<R>(runner: &mut R, program: &str, args: Vec<String>) -> Result<(), String>
where
    R: CommandRunner,
{
    runner.run(program, &args)
}

fn run_shell_command<R>(runner: &mut R, command: &str, context: &Context) -> Result<(), String>
where
    R: CommandRunner,
{
    let rendered = render_shell_template(command, context)?;
    let rendered = append_file_argument_if_needed(command, rendered, context);
    runner.run("sh", &["-c".into(), rendered])
}

fn run_template<R>(
    runner: &mut R,
    template: Vec<String>,
    replacements: &[(&str, &str)],
) -> Result<(), String>
where
    R: CommandRunner,
{
    let mut parts = template
        .into_iter()
        .map(|part| replace_placeholders(&part, replacements))
        .collect::<Vec<_>>();

    if parts.is_empty() {
        return Err("configured command cannot be empty".into());
    }

    let program = parts.remove(0);
    runner.run(&program, &parts)
}

fn replace_placeholders(template: &str, replacements: &[(&str, &str)]) -> String {
    let mut rendered = template.to_string();
    for (placeholder, value) in replacements {
        rendered = rendered.replace(placeholder, value);
    }
    rendered
}

fn render_quickmenu_template(template: &str, context: &Context) -> Result<String, String> {
    let rendered = render_quickmenu_command_template(template);
    let text = context_text(context).unwrap_or("");
    let path = context_path(context).unwrap_or("");
    let directory = context_directory(context).unwrap_or("");

    Ok(replace_placeholders(
        &rendered,
        &[
            ("{text}", text),
            ("{path}", path),
            ("{directory}", directory),
            ("%s", text),
            ("%f", path),
        ],
    ))
}

fn render_shell_template(template: &str, context: &Context) -> Result<String, String> {
    let rendered = render_quickmenu_command_template(template);
    let text = context_text(context).unwrap_or("");
    let path = context_path(context).unwrap_or("");
    let quoted_path = shell_quote(path);
    let directory = context_directory(context).unwrap_or("");
    let quoted_directory = shell_quote(directory);

    Ok(replace_placeholders(
        &rendered,
        &[
            ("{text}", text),
            ("{path}", quoted_path.as_str()),
            ("{directory}", quoted_directory.as_str()),
            ("%s", text),
            ("%f", quoted_path.as_str()),
        ],
    ))
}

fn render_quickmenu_url(url: &str, context: &Context) -> Result<String, String> {
    let rendered = render_quickmenu_command_template(url);
    let text = context_text(context).unwrap_or("");
    let encoded_text = urlencoding::encode(text);
    let path = context_path(context).unwrap_or("");
    let directory = context_directory(context).unwrap_or("");

    Ok(replace_placeholders(
        &rendered,
        &[
            ("{text}", text),
            ("{query}", encoded_text.as_ref()),
            ("{path}", path),
            ("{directory}", directory),
            ("%s", encoded_text.as_ref()),
            ("%f", path),
        ],
    ))
}

fn append_file_argument_if_needed(command: &str, rendered: String, context: &Context) -> String {
    if !matches!(context.kind, ContextKind::File | ContextKind::Path)
        || command_contains_file_placeholder(command)
    {
        return rendered;
    }

    let Ok(path) = context_path(context) else {
        return rendered;
    };

    format!("{} {}", rendered, shell_quote(path))
}

fn command_contains_file_placeholder(command: &str) -> bool {
    command.contains("%f") || command.contains("{path}") || command.contains("{directory}")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn render_quickmenu_path(path: &str, context: &Context) -> Result<String, String> {
    let rendered = render_quickmenu_template(path, context)?;
    expand_home_path(&rendered)
}

fn expand_home_path(path: &str) -> Result<String, String> {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "HOME is not set".to_string())?;
        return Ok(home.join(rest).to_string_lossy().into_owned());
    }

    Ok(path.to_string())
}

fn browser_program(config: &LoadedConfig) -> &str {
    config
        .app
        .general
        .as_ref()
        .and_then(|general| general.default_browser_opener.as_deref())
        .unwrap_or("xdg-open")
}

fn configured_copy_command(config: &LoadedConfig) -> Vec<String> {
    config
        .app
        .actions
        .as_ref()
        .and_then(|actions| actions.copy_command.clone())
        .unwrap_or_else(|| vec!["wl-copy".into(), "{text}".into()])
}

fn configured_reveal_command(config: &LoadedConfig) -> Vec<String> {
    config
        .app
        .actions
        .as_ref()
        .and_then(|actions| actions.reveal_command.clone())
        .unwrap_or_else(|| vec!["dolphin".into(), "--select".into(), "{path}".into()])
}

fn configured_open_directory_program(config: &LoadedConfig) -> &str {
    config
        .app
        .actions
        .as_ref()
        .and_then(|actions| actions.open_directory_command.as_ref())
        .and_then(|command| command.first())
        .map(String::as_str)
        .unwrap_or("xdg-open")
}

fn configured_open_directory_args(config: &LoadedConfig, directory: &str) -> Vec<String> {
    if let Some(command) = config
        .app
        .actions
        .as_ref()
        .and_then(|actions| actions.open_directory_command.as_ref())
    {
        return command
            .iter()
            .skip(1)
            .map(|part| part.replace("{directory}", directory))
            .collect();
    }

    vec![directory.to_string()]
}

fn context_url(context: &Context) -> Result<&str, String> {
    context
        .text
        .as_deref()
        .filter(|_| context.kind == ContextKind::Url)
        .ok_or_else(|| "open-url action requires url context".to_string())
}

fn context_text(context: &Context) -> Result<&str, String> {
    context
        .text
        .as_deref()
        .or_else(|| (!context.raw.is_empty()).then_some(context.raw.as_str()))
        .ok_or_else(|| "action requires text input".to_string())
}

fn context_path(context: &Context) -> Result<&str, String> {
    context
        .files
        .first()
        .and_then(|path| path.to_str())
        .ok_or_else(|| "action requires file path context".to_string())
}

fn context_directory(context: &Context) -> Result<&str, String> {
    match context.kind {
        ContextKind::Directory => context_path(context),
        _ => context
            .files
            .first()
            .and_then(|path| path.parent())
            .and_then(|path| path.to_str())
            .ok_or_else(|| "action requires directory context".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandRunner, execute, expand_home_path, render_shell_template};
    use crate::config::{
        ActionsConfig, AppConfig, LoadedConfig, MenuConfig, MenusConfig, ProviderConfig,
        ResolvedConfigPaths,
    };
    use crate::context::{Context, ContextKind, ContextSource};
    use crate::menu::ActionCommand;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[derive(Default)]
    struct RecordingRunner {
        invocations: Vec<(String, Vec<String>)>,
    }

    impl CommandRunner for RecordingRunner {
        fn run(&mut self, program: &str, args: &[String]) -> Result<(), String> {
            self.invocations.push((program.to_string(), args.to_vec()));
            Ok(())
        }
    }

    #[test]
    fn opens_url_with_configured_browser_program() {
        let config = sample_config();
        let context = Context {
            source: ContextSource::ExplicitText,
            kind: ContextKind::Url,
            raw: "https://example.com".into(),
            text: Some("https://example.com".into()),
            files: Vec::new(),
            mime_types: Vec::new(),
        };
        let mut runner = RecordingRunner::default();

        execute(&config, &context, &ActionCommand::OpenUrl, &mut runner).unwrap();

        assert_eq!(
            runner.invocations,
            vec![("browser-open".into(), vec!["https://example.com".into()])]
        );
    }

    #[test]
    fn builds_search_url_from_provider_template() {
        let config = sample_config();
        let context = Context {
            source: ContextSource::ExplicitText,
            kind: ContextKind::Text,
            raw: "hello world".into(),
            text: Some("hello world".into()),
            files: Vec::new(),
            mime_types: Vec::new(),
        };
        let mut runner = RecordingRunner::default();

        execute(
            &config,
            &context,
            &ActionCommand::Search {
                provider: "duckduckgo".into(),
            },
            &mut runner,
        )
        .unwrap();

        assert_eq!(
            runner.invocations,
            vec![(
                "browser-open".into(),
                vec!["https://duckduckgo.com/?q=hello%20world".into()],
            )]
        );
    }

    #[test]
    fn copies_text_with_configured_copy_command() {
        let config = sample_config();
        let context = Context {
            source: ContextSource::ExplicitText,
            kind: ContextKind::Text,
            raw: "hello world".into(),
            text: Some("hello world".into()),
            files: Vec::new(),
            mime_types: Vec::new(),
        };
        let mut runner = RecordingRunner::default();

        execute(&config, &context, &ActionCommand::CopyText, &mut runner).unwrap();

        assert_eq!(
            runner.invocations,
            vec![("copy-tool".into(), vec!["hello world".into()])]
        );
    }

    #[test]
    fn runs_shell_action_through_sh() {
        let config = sample_config();
        let context = Context {
            source: ContextSource::ExplicitFiles,
            kind: ContextKind::File,
            raw: "/tmp/note.txt".into(),
            text: None,
            files: vec![PathBuf::from("/tmp/note.txt")],
            mime_types: Vec::new(),
        };
        let mut runner = RecordingRunner::default();

        execute(
            &config,
            &context,
            &ActionCommand::Shell {
                command: "code %f".into(),
            },
            &mut runner,
        )
        .unwrap();

        assert_eq!(
            runner.invocations,
            vec![(
                "sh".into(),
                vec!["-c".into(), "code '/tmp/note.txt'".into()]
            )]
        );
    }

    #[test]
    fn opens_quickmenu_url_with_text_placeholder() {
        let config = sample_config();
        let context = Context {
            source: ContextSource::ExplicitText,
            kind: ContextKind::Text,
            raw: "hello world".into(),
            text: Some("hello world".into()),
            files: Vec::new(),
            mime_types: Vec::new(),
        };
        let mut runner = RecordingRunner::default();

        execute(
            &config,
            &context,
            &ActionCommand::QuickUrl {
                url: "https://example.com?q=%s".into(),
            },
            &mut runner,
        )
        .unwrap();

        assert_eq!(
            runner.invocations,
            vec![(
                "browser-open".into(),
                vec!["https://example.com?q=hello%20world".into()]
            )]
        );
    }

    #[test]
    fn appends_file_path_to_shell_action_without_file_placeholder() {
        let config = sample_config();
        let context = Context {
            source: ContextSource::ExplicitFiles,
            kind: ContextKind::File,
            raw: "/tmp/note file.txt".into(),
            text: None,
            files: vec![PathBuf::from("/tmp/note file.txt")],
            mime_types: Vec::new(),
        };
        let mut runner = RecordingRunner::default();

        execute(
            &config,
            &context,
            &ActionCommand::Shell {
                command: "wps".into(),
            },
            &mut runner,
        )
        .unwrap();

        assert_eq!(
            runner.invocations,
            vec![(
                "sh".into(),
                vec!["-c".into(), "wps '/tmp/note file.txt'".into()]
            )]
        );
    }

    #[test]
    fn quotes_file_placeholder_in_shell_action() {
        let config = sample_config();
        let context = Context {
            source: ContextSource::ExplicitFiles,
            kind: ContextKind::File,
            raw: "/tmp/image file.png".into(),
            text: None,
            files: vec![PathBuf::from("/tmp/image file.png")],
            mime_types: Vec::new(),
        };
        let mut runner = RecordingRunner::default();

        execute(
            &config,
            &context,
            &ActionCommand::Shell {
                command: "qimgv %f".into(),
            },
            &mut runner,
        )
        .unwrap();

        assert_eq!(
            runner.invocations,
            vec![(
                "sh".into(),
                vec!["-c".into(), "qimgv '/tmp/image file.png'".into()]
            )]
        );
    }

    #[test]
    fn opens_quickmenu_path_with_home_expansion() {
        let config = sample_config();
        let context = Context {
            source: ContextSource::Empty,
            kind: ContextKind::Empty,
            raw: String::new(),
            text: None,
            files: Vec::new(),
            mime_types: Vec::new(),
        };
        let mut runner = RecordingRunner::default();

        execute(
            &config,
            &context,
            &ActionCommand::QuickPath {
                path: "~/Apps".into(),
            },
            &mut runner,
        )
        .unwrap();

        let expected = expand_home_path("~/Apps").unwrap();
        assert_eq!(
            runner.invocations,
            vec![("xdg-open".into(), vec![expected])]
        );
    }

    #[test]
    fn renders_quickmenu_shell_placeholders() {
        let context = Context {
            source: ContextSource::ExplicitFiles,
            kind: ContextKind::File,
            raw: "/tmp/note.txt".into(),
            text: Some("hello world".into()),
            files: vec![PathBuf::from("/tmp/note.txt")],
            mime_types: Vec::new(),
        };

        let rendered = render_shell_template("echo %s %f {directory}", &context).unwrap();

        assert_eq!(rendered, "echo hello world '/tmp/note.txt' '/tmp'");
    }

    #[test]
    fn separator_action_is_noop() {
        let config = sample_config();
        let context = Context {
            source: ContextSource::Empty,
            kind: ContextKind::Empty,
            raw: String::new(),
            text: None,
            files: Vec::new(),
            mime_types: Vec::new(),
        };
        let mut runner = RecordingRunner::default();

        execute(&config, &context, &ActionCommand::Separator, &mut runner).unwrap();

        assert!(runner.invocations.is_empty());
    }

    fn sample_config() -> LoadedConfig {
        let mut providers = BTreeMap::new();
        providers.insert(
            "duckduckgo".into(),
            ProviderConfig {
                label: "DuckDuckGo".into(),
                url: "https://duckduckgo.com/?q={query}".into(),
                default: Some(true),
            },
        );

        LoadedConfig {
            app: AppConfig {
                general: Some(crate::config::GeneralConfig {
                    file_manager: None,
                    terminal: None,
                    default_browser_opener: Some("browser-open".into()),
                    default_mode: None,
                    no_context_behavior: None,
                    prefer_primary_selection: None,
                    fallback_to_clipboard: None,
                }),
                ui: None,
                actions: Some(ActionsConfig {
                    reveal_command: None,
                    open_directory_command: None,
                    copy_command: Some(vec!["copy-tool".into(), "{text}".into()]),
                }),
                menus: Some(MenusConfig {
                    default_file: None,
                    default_id: None,
                    fallback_id: None,
                }),
                providers,
                rules: Vec::new(),
            },
            menu: MenuConfig { menus: Vec::new() },
            sources: ResolvedConfigPaths {
                config_toml: PathBuf::from("config.toml"),
                menu_file: PathBuf::from("menu.ini"),
            },
        }
    }
}
