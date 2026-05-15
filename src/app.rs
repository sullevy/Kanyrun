use crate::actions::{CommandRunner, SystemCommandRunner};
use crate::cli::{CliRequest, ParseOutcome};
use crate::config::{LoadedConfig, config_paths, load};
use crate::context::{Context, ContextSource};
use crate::detect::{SelectionReader, resolve_context};
use crate::menu::MenuAction;
use crate::ui::MenuPresenter;
use std::process::Command;

pub fn run() -> Result<(), String> {
    let request = match crate::cli::parse()? {
        ParseOutcome::Request(request) => request,
        ParseOutcome::Help(help) => {
            println!("{help}");
            return Ok(());
        }
    };

    if request.daemon_server {
        return crate::daemon::serve(&request);
    }

    if request.oneshot {
        return run_local(&request);
    }

    crate::daemon::dispatch(request)
}

pub(crate) fn run_local(request: &CliRequest) -> Result<(), String> {
    let config = load(&config_paths(), request.menu_file.as_deref())?;
    let reader = WlPasteSelectionReader;
    let mut presenter = crate::ui::DefaultPresenter::from_config(&config)?;
    let mut runner = SystemCommandRunner;

    run_with(request, &config, &reader, &mut presenter, &mut runner)
}

pub(crate) fn run_with<R, S, P>(
    request: &CliRequest,
    config: &LoadedConfig,
    reader: &S,
    presenter: &mut P,
    runner: &mut R,
) -> Result<(), String>
where
    R: CommandRunner,
    S: SelectionReader,
    P: MenuPresenter,
{
    let context = if request.test_menu {
        Context::empty(ContextSource::Empty)
    } else {
        resolve_context(request, reader)?
    };
    let resolution = if request.test_menu {
        crate::rules::Resolution::Menu(default_test_menu(config, &context)?)
    } else {
        crate::rules::resolve(config, &context, request.force_menu)?
    };

    match resolution {
        crate::rules::Resolution::Direct(command) => crate::actions::execute(config, &context, &command, runner),
        crate::rules::Resolution::Menu(menu) => {
            if request.test_menu {
                presenter.show_menu(&menu)?;
                Ok(())
            } else if let Some(action_id) = presenter.show_menu(&menu)? {
                let action = find_action(&menu.actions, &action_id)?;
                let command = action
                    .command
                    .as_ref()
                    .ok_or_else(|| format!("menu action is not executable: {action_id}"))?;
                crate::actions::execute(config, &context, command, runner)
            } else {
                Ok(())
            }
        }
        crate::rules::Resolution::None => Ok(()),
    }
}

fn find_action<'a>(actions: &'a [MenuAction], action_id: &str) -> Result<&'a MenuAction, String> {
    for action in actions {
        if action.id == action_id {
            return Ok(action);
        }

        if let Some(submenu) = action.submenu.as_ref()
            && let Ok(found) = find_action(submenu, action_id)
        {
            return Ok(found);
        }
    }

    Err(format!("menu returned unknown action id: {action_id}"))
}

fn default_test_menu(config: &LoadedConfig, context: &Context) -> Result<crate::menu::MenuModel, String> {
    let menu_id = config
        .app
        .menus
        .as_ref()
        .and_then(|menus| menus.default_id.as_deref())
        .or_else(|| config.menu.menus.first().map(|menu| menu.id.as_str()))
        .ok_or_else(|| "no menus configured".to_string())?;

    crate::menu::build_model(config, menu_id, context)
}

pub(crate) struct WlPasteSelectionReader;

impl SelectionReader for WlPasteSelectionReader {
    fn read_clipboard(&self) -> Result<Option<String>, String> {
        read_wl_paste(&[])
    }

    fn read_primary(&self) -> Result<Option<String>, String> {
        read_wl_paste(&["--primary"])
    }
}

fn read_wl_paste(args: &[&str]) -> Result<Option<String>, String> {
    let output = match Command::new("wl-paste").args(args).output() {
        Ok(output) => output,
        Err(_) => return Ok(None),
    };

    if !output.status.success() {
        return Ok(None);
    }

    Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::run_with;
    use crate::actions::CommandRunner;
    use crate::cli::CliRequest;
    use crate::config::{AppConfig, GeneralConfig, LoadedConfig, MenuActionCommandConfig, MenuActionConfig, MenuConfig, MenuDefinition, MenusConfig, ProviderConfig, ResolvedConfigPaths, RuleConfig, RuleMatch};
    use crate::detect::SelectionReader;
    use crate::menu::MenuModel;
    use crate::ui::MenuPresenter;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    struct FakeSelectionReader;

    impl SelectionReader for FakeSelectionReader {
        fn read_clipboard(&self) -> Result<Option<String>, String> {
            Ok(None)
        }

        fn read_primary(&self) -> Result<Option<String>, String> {
            Ok(None)
        }
    }

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

    struct SelectingPresenter;

    impl MenuPresenter for SelectingPresenter {
        fn show_menu(&mut self, menu: &MenuModel) -> Result<Option<String>, String> {
            if let Some(submenu) = menu.actions.first().and_then(|action| action.submenu.as_ref()) {
                return Ok(submenu.first().map(|action| action.id.clone()));
            }

            Ok(menu.actions.first().map(|action| action.id.clone()))
        }
    }

    struct ClosingPresenter;

    impl MenuPresenter for ClosingPresenter {
        fn show_menu(&mut self, _menu: &MenuModel) -> Result<Option<String>, String> {
            Ok(None)
        }
    }

    #[test]
    fn executes_direct_url_rule_end_to_end() {
        let config = sample_config();
        let request = CliRequest {
            text: Some("https://example.com".into()),
            ..CliRequest::default()
        };
        let mut runner = RecordingRunner::default();
        let mut presenter = SelectingPresenter;

        run_with(&request, &config, &FakeSelectionReader, &mut presenter, &mut runner).unwrap();

        assert_eq!(runner.invocations, vec![("browser-open".into(), vec!["https://example.com".into()])]);
    }

    #[test]
    fn executes_default_text_menu_action_end_to_end() {
        let config = sample_config();
        let request = CliRequest {
            text: Some("hello world".into()),
            ..CliRequest::default()
        };
        let mut runner = RecordingRunner::default();
        let mut presenter = SelectingPresenter;

        run_with(&request, &config, &FakeSelectionReader, &mut presenter, &mut runner).unwrap();

        assert_eq!(
            runner.invocations,
            vec![(
                "browser-open".into(),
                vec!["https://duckduckgo.com/?q=hello%20world".into()],
            )]
        );
    }

    #[test]
    fn test_flag_forces_first_configured_menu() {
        let config = sample_config();
        let request = CliRequest {
            test_menu: true,
            ..CliRequest::default()
        };
        let mut runner = RecordingRunner::default();
        let mut presenter = ClosingPresenter;

        run_with(&request, &config, &FakeSelectionReader, &mut presenter, &mut runner).unwrap();

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
        providers.insert(
            "github".into(),
            ProviderConfig {
                label: "GitHub".into(),
                url: "https://github.com/search?q={query}".into(),
                default: None,
            },
        );

        LoadedConfig {
            app: AppConfig {
                general: Some(GeneralConfig {
                    file_manager: None,
                    terminal: None,
                    default_browser_opener: Some("browser-open".into()),
                    default_mode: None,
                    no_context_behavior: None,
                    prefer_primary_selection: None,
                    fallback_to_clipboard: None,
                }),
                ui: None,
                actions: None,
                menus: Some(MenusConfig {
                    default_file: None,
                    default_id: Some("text".into()),
                    fallback_id: Some("text".into()),
                }),
                providers,
                rules: vec![
                    RuleConfig {
                        when: RuleMatch {
                            kind: Some("url".into()),
                            source: None,
                            mime: None,
                            mime_glob: None,
                            extensions: None,
                            is_multi: None,
                        },
                        behavior: "direct".into(),
                        action: Some("open-url".into()),
                        menu: None,
                    },
                    RuleConfig {
                        when: RuleMatch {
                            kind: Some("text".into()),
                            source: None,
                            mime: None,
                            mime_glob: None,
                            extensions: None,
                            is_multi: None,
                        },
                        behavior: "menu".into(),
                        action: None,
                        menu: Some("text".into()),
                    },
                ],
            },
            menu: MenuConfig {
                menus: vec![
                    MenuDefinition {
                        id: "text".into(),
                        title: "Text".into(),
                        extensions: Vec::new(),
                        context_default: false,
                        actions: vec![
                            MenuActionConfig {
                                id: "search".into(),
                                label: "Search".into(),
                                icon: None,
                                default: Some(true),
                                submenu: Some("search-providers".into()),
                                command: None,
                            },
                            MenuActionConfig {
                                id: "copy-text".into(),
                                label: "Copy text".into(),
                                icon: None,
                                default: None,
                                submenu: None,
                                command: Some(MenuActionCommandConfig {
                                    kind: "copy-text".into(),
                                    provider: None,
                                    desktop_id: None,
                                }),
                            },
                        ],
                    },
                    MenuDefinition {
                        id: "search-providers".into(),
                        title: "Search Providers".into(),
                        extensions: Vec::new(),
                        context_default: false,
                        actions: vec![
                            MenuActionConfig {
                                id: "search-default".into(),
                                label: "Search: {text_preview}".into(),
                                icon: None,
                                default: Some(true),
                                submenu: None,
                                command: Some(MenuActionCommandConfig {
                                    kind: "search".into(),
                                    provider: Some("duckduckgo".into()),
                                    desktop_id: None,
                                }),
                            },
                            MenuActionConfig {
                                id: "search-github".into(),
                                label: "GitHub search".into(),
                                icon: None,
                                default: None,
                                submenu: None,
                                command: Some(MenuActionCommandConfig {
                                    kind: "search".into(),
                                    provider: Some("github".into()),
                                    desktop_id: None,
                                }),
                            },
                        ],
                    },
                ],
            },
            sources: ResolvedConfigPaths {
                config_toml: PathBuf::from("config.toml"),
                menu_file: PathBuf::from("menu.ini"),
            },
        }
    }
}
