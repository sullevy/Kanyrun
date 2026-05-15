use crate::config::LoadedConfig;
use crate::context::Context;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    pub id: String,
    pub title: String,
    pub actions: Vec<MenuAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuAction {
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    pub is_default: bool,
    pub command: Option<ActionCommand>,
    pub submenu: Option<Vec<MenuAction>>,
    pub is_separator: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionCommand {
    OpenUrl,
    OpenFile,
    OpenDirectory,
    RevealFile,
    OpenWith { desktop_id: String },
    Search { provider: String },
    CopyText,
    CopyPath,
    QuickUrl { url: String },
    QuickPath { path: String },
    Shell { command: String },
    Separator,
}

pub fn build_model(config: &LoadedConfig, menu_id: &str, context: &Context) -> Result<MenuModel, String> {
    let menu = config
        .menu
        .menus
        .iter()
        .find(|menu| menu.id == menu_id)
        .ok_or_else(|| format!("unknown menu: {menu_id}"))?;

    if !menu_matches_context(menu, context) {
        return Ok(MenuModel {
            id: menu.id.clone(),
            title: menu.title.clone(),
            actions: Vec::new(),
        });
    }

    Ok(MenuModel {
        id: menu.id.clone(),
        title: menu.title.clone(),
        actions: build_actions(config, &menu.actions, context, is_default_menu_id(config, menu_id))?,
    })
}

#[cfg(test)]
pub fn default_action_id(menu: &MenuModel) -> Option<&str> {
    find_default_action(&menu.actions)
}

#[cfg(test)]
fn find_default_action(actions: &[MenuAction]) -> Option<&str> {
    for action in actions {
        if action.is_default && action.command.is_some() && !action.is_separator {
            return Some(action.id.as_str());
        }

        if let Some(submenu) = action.submenu.as_ref()
            && let Some(id) = find_default_action(submenu)
        {
            return Some(id);
        }
    }

    None
}

fn build_actions(
    config: &LoadedConfig,
    actions: &[crate::config::MenuActionConfig],
    context: &Context,
    hide_context_default_submenus: bool,
) -> Result<Vec<MenuAction>, String> {
    let mut built = Vec::with_capacity(actions.len());
    let mut default_count = 0;

    for action in actions {
        let has_command = action.command.is_some();
        let has_submenu = action.submenu.is_some();
        if has_command == has_submenu {
            return Err(format!(
                "action {} must define exactly one of command or submenu",
                action.id
            ));
        }

        let command = action
            .command
            .as_ref()
            .map(|command| parse_command(config, command))
            .transpose()?;
        let is_separator = matches!(command, Some(ActionCommand::Separator));

        let is_default = action.default.unwrap_or(false);
        if is_default && !is_separator {
            default_count += 1;
        }

        let submenu = match action.submenu.as_deref() {
            Some(submenu_id) => {
                let submenu = config
                    .menu
                    .menus
                    .iter()
                    .find(|menu| menu.id == submenu_id)
                    .ok_or_else(|| format!("unknown menu: {submenu_id}"))?;
                if should_hide_submenu(submenu, context, hide_context_default_submenus) {
                    continue;
                }
                Some(build_actions(
                    config,
                    &submenu.actions,
                    context,
                    hide_context_default_submenus,
                )?)
            }
            None => None,
        };

        built.push(MenuAction {
            id: action.id.clone(),
            label: render_label(&action.label, context),
            icon: action.icon.clone(),
            is_default,
            command,
            submenu,
            is_separator,
        });
    }

    if default_count > 1 {
        return Err("menu has multiple default actions".into());
    }

    Ok(built)
}

fn should_hide_submenu(
    menu: &crate::config::MenuDefinition,
    context: &Context,
    hide_context_default_submenus: bool,
) -> bool {
    (hide_context_default_submenus && menu.context_default) || !menu_matches_context(menu, context)
}

fn is_default_menu_id(config: &LoadedConfig, menu_id: &str) -> bool {
    config
        .app
        .menus
        .as_ref()
        .and_then(|menus| menus.default_id.as_deref())
        .or_else(|| config.menu.menus.first().map(|menu| menu.id.as_str()))
        == Some(menu_id)
}

fn menu_matches_context(menu: &crate::config::MenuDefinition, context: &Context) -> bool {
    if menu.extensions.is_empty() {
        return true;
    }

    context.files.iter().any(|path| {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| menu.extensions.iter().any(|candidate| candidate.eq_ignore_ascii_case(ext)))
            .unwrap_or(false)
    })
}

fn parse_command(
    config: &LoadedConfig,
    command: &crate::config::MenuActionCommandConfig,
) -> Result<ActionCommand, String> {
    match command.kind.as_str() {
        "open-url" => Ok(ActionCommand::OpenUrl),
        "open-file" => Ok(ActionCommand::OpenFile),
        "open-directory" => Ok(ActionCommand::OpenDirectory),
        "reveal-file" => Ok(ActionCommand::RevealFile),
        "copy-text" => Ok(ActionCommand::CopyText),
        "copy-path" => Ok(ActionCommand::CopyPath),
        "separator" => Ok(ActionCommand::Separator),
        "quick-url" => {
            let url = command
                .provider
                .as_ref()
                .ok_or_else(|| "quick-url action missing url".to_string())?;
            Ok(ActionCommand::QuickUrl { url: url.clone() })
        }
        "quick-path" => {
            let path = command
                .provider
                .as_ref()
                .ok_or_else(|| "quick-path action missing path".to_string())?;
            Ok(ActionCommand::QuickPath { path: path.clone() })
        }
        "shell" => {
            let shell_command = command
                .provider
                .as_ref()
                .ok_or_else(|| "shell action missing command".to_string())?;
            Ok(ActionCommand::Shell {
                command: shell_command.clone(),
            })
        }
        "search" => {
            let provider = command
                .provider
                .as_ref()
                .ok_or_else(|| "search action missing provider".to_string())?;

            if !config.app.providers.contains_key(provider) {
                return Err(format!("unknown provider: {provider}"));
            }

            Ok(ActionCommand::Search {
                provider: provider.clone(),
            })
        }
        "open-with" => {
            let desktop_id = command
                .desktop_id
                .as_ref()
                .ok_or_else(|| "open-with action missing desktop_id".to_string())?;
            Ok(ActionCommand::OpenWith {
                desktop_id: desktop_id.clone(),
            })
        }
        other => Err(format!("unsupported action type: {other}")),
    }
}

fn render_label(template: &str, context: &Context) -> String {
    template.replace("{text_preview}", &text_preview(context))
}

fn text_preview(context: &Context) -> String {
    context
        .text
        .as_deref()
        .unwrap_or_default()
        .trim()
        .chars()
        .take(40)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ActionCommand, build_model, default_action_id};
    use crate::config::{AppConfig, LoadedConfig, MenuActionCommandConfig, MenuActionConfig, MenuConfig, MenuDefinition, MenusConfig, ResolvedConfigPaths};
    use crate::context::{Context, ContextKind, ContextSource};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn builds_menu_in_declared_order_and_substitutes_text_preview() {
        let config = sample_loaded_config();
        let context = Context {
            source: ContextSource::ExplicitText,
            kind: ContextKind::Text,
            raw: "hello world".into(),
            text: Some("hello world".into()),
            files: Vec::new(),
            mime_types: Vec::new(),
        };

        let menu = build_model(&config, "text", &context).unwrap();

        assert_eq!(menu.actions.len(), 2);
        assert_eq!(menu.actions[0].id, "search-default");
        assert_eq!(menu.actions[0].label, "Search: hello world");
        assert_eq!(menu.actions[1].id, "copy-text");
    }

    #[test]
    fn validates_search_provider_references() {
        let mut config = sample_loaded_config();
        config.menu.menus[0].actions[0].command.as_mut().unwrap().provider = Some("missing".into());
        let context = text_context();

        let error = build_model(&config, "text", &context).unwrap_err();

        assert!(error.contains("missing"));
    }

    #[test]
    fn returns_declared_default_action() {
        let config = sample_loaded_config();
        let context = text_context();
        let menu = build_model(&config, "text", &context).unwrap();

        assert_eq!(default_action_id(&menu), Some("search-default"));
    }

    #[test]
    fn supports_second_level_submenus() {
        let config = submenu_loaded_config();
        let context = text_context();

        let menu = build_model(&config, "text", &context).unwrap();

        assert_eq!(menu.actions.len(), 2);
        assert_eq!(menu.actions[0].id, "search");
        assert!(menu.actions[0].command.is_none());
        let submenu = menu.actions[0].submenu.as_ref().unwrap();
        assert_eq!(submenu.len(), 2);
        assert_eq!(submenu[0].id, "search-default");
        assert_eq!(submenu[0].label, "Search: hello world");
        assert_eq!(submenu[1].id, "search-github");
    }

    #[test]
    fn returns_default_action_from_submenu() {
        let config = submenu_loaded_config();
        let context = text_context();
        let menu = build_model(&config, "text", &context).unwrap();

        assert_eq!(default_action_id(&menu), Some("search-default"));
    }

    #[test]
    fn rejects_action_with_both_command_and_submenu() {
        let mut config = sample_loaded_config();
        config.menu.menus[0].actions[0].submenu = Some("search-providers".into());
        let context = text_context();

        let error = build_model(&config, "text", &context).unwrap_err();

        assert!(error.contains("must define exactly one of command or submenu"));
    }

    #[test]
    fn supports_separator_items() {
        let mut config = sample_loaded_config();
        config.menu.menus[0].actions.push(MenuActionConfig {
            id: "separator-1".into(),
            label: "---".into(),
            icon: None,
            default: None,
            submenu: None,
            command: Some(MenuActionCommandConfig {
                kind: "separator".into(),
                provider: None,
                desktop_id: None,
            }),
        });

        let menu = build_model(&config, "text", &text_context()).unwrap();

        assert!(menu.actions[2].is_separator);
        assert_eq!(menu.actions[2].command, Some(ActionCommand::Separator));
    }

    #[test]
    fn supports_shell_actions() {
        let mut config = sample_loaded_config();
        config.menu.menus[0].actions[0].command = Some(MenuActionCommandConfig {
            kind: "shell".into(),
            provider: Some("code {path}".into()),
            desktop_id: None,
        });

        let menu = build_model(&config, "text", &text_context()).unwrap();

        assert_eq!(
            menu.actions[0].command,
            Some(ActionCommand::Shell {
                command: "code {path}".into(),
            })
        );
    }

    #[test]
    fn supports_quickmenu_url_and_path_actions() {
        let mut config = sample_loaded_config();
        config.menu.menus[0].actions = vec![
            MenuActionConfig {
                id: "open-url".into(),
                label: "Open URL".into(),
                icon: None,
                default: None,
                submenu: None,
                command: Some(MenuActionCommandConfig {
                    kind: "quick-url".into(),
                    provider: Some("https://example.com?q=%s".into()),
                    desktop_id: None,
                }),
            },
            MenuActionConfig {
                id: "open-path".into(),
                label: "Open Path".into(),
                icon: None,
                default: None,
                submenu: None,
                command: Some(MenuActionCommandConfig {
                    kind: "quick-path".into(),
                    provider: Some("~/Apps".into()),
                    desktop_id: None,
                }),
            },
        ];

        let menu = build_model(&config, "text", &text_context()).unwrap();

        assert_eq!(
            menu.actions[0].command,
            Some(ActionCommand::QuickUrl {
                url: "https://example.com?q=%s".into(),
            })
        );
        assert_eq!(
            menu.actions[1].command,
            Some(ActionCommand::QuickPath {
                path: "~/Apps".into(),
            })
        );
    }

    #[test]
    fn hides_extension_gated_submenu_when_context_does_not_match() {
        let mut config = sample_loaded_config();
        config.menu.menus = vec![
            MenuDefinition {
                id: "root".into(),
                title: "Root".into(),
                extensions: Vec::new(),
                context_default: false,
                actions: vec![MenuActionConfig {
                    id: "python-tools".into(),
                    label: "Python tools".into(),
                    icon: None,
                    default: None,
                    submenu: Some("python".into()),
                    command: None,
                }],
            },
            MenuDefinition {
                id: "python".into(),
                title: "Python".into(),
                context_default: false,
                extensions: vec!["py".into()],
                actions: vec![MenuActionConfig {
                    id: "open-file".into(),
                    label: "Open".into(),
                    icon: None,
                    default: Some(true),
                    submenu: None,
                    command: Some(MenuActionCommandConfig {
                        kind: "open-file".into(),
                        provider: None,
                        desktop_id: None,
                    }),
                }],
            },
        ];

        let context = Context {
            source: ContextSource::ExplicitFiles,
            kind: ContextKind::File,
            raw: "/tmp/readme.txt".into(),
            text: None,
            files: vec![PathBuf::from("/tmp/readme.txt")],
            mime_types: vec!["text/plain".into()],
        };

        let menu = build_model(&config, "root", &context).unwrap();

        assert!(menu.actions.is_empty());
    }

    #[test]
    fn keeps_unconditional_root_items_when_extension_submenu_is_filtered() {
        let mut config = sample_loaded_config();
        config.menu.menus = vec![
            MenuDefinition {
                id: "root".into(),
                title: "Root".into(),
                extensions: Vec::new(),
                context_default: false,
                actions: vec![
                    MenuActionConfig {
                        id: "google".into(),
                        label: "谷歌".into(),
                        icon: None,
                        default: None,
                        submenu: None,
                        command: Some(MenuActionCommandConfig {
                            kind: "quick-url".into(),
                            provider: Some("https://www.google.com/search?q=%s".into()),
                            desktop_id: None,
                        }),
                    },
                    MenuActionConfig {
                        id: "common".into(),
                        label: "常用".into(),
                        icon: None,
                        default: None,
                        submenu: Some("common".into()),
                        command: None,
                    },
                    MenuActionConfig {
                        id: "image".into(),
                        label: "图片".into(),
                        icon: None,
                        default: None,
                        submenu: Some("image".into()),
                        command: None,
                    },
                ],
            },
            MenuDefinition {
                id: "common".into(),
                title: "常用".into(),
                extensions: Vec::new(),
                context_default: false,
                actions: vec![MenuActionConfig {
                    id: "apps".into(),
                    label: "Apps".into(),
                    icon: None,
                    default: None,
                    submenu: None,
                    command: Some(MenuActionCommandConfig {
                        kind: "quick-path".into(),
                        provider: Some("~/Apps".into()),
                        desktop_id: None,
                    }),
                }],
            },
            MenuDefinition {
                id: "image".into(),
                title: "图片".into(),
                context_default: false,
                extensions: vec!["png".into()],
                actions: vec![MenuActionConfig {
                    id: "qimgv".into(),
                    label: "qimgv".into(),
                    icon: None,
                    default: None,
                    submenu: None,
                    command: Some(MenuActionCommandConfig {
                        kind: "shell".into(),
                        provider: Some("qimgv %f".into()),
                        desktop_id: None,
                    }),
                }],
            },
        ];

        let context = Context {
            source: ContextSource::ExplicitFiles,
            kind: ContextKind::File,
            raw: "/tmp/readme.txt".into(),
            text: None,
            files: vec![PathBuf::from("/tmp/readme.txt")],
            mime_types: vec!["text/plain".into()],
        };

        let menu = build_model(&config, "root", &context).unwrap();

        assert_eq!(menu.actions.len(), 2);
        assert_eq!(menu.actions[0].id, "google");
        assert_eq!(menu.actions[1].id, "common");
    }

    #[test]
    fn hides_context_default_submenus_from_default_menu() {
        let mut config = sample_loaded_config();
        config.app.menus = Some(MenusConfig {
            default_file: None,
            default_id: Some("root".into()),
            fallback_id: None,
        });
        config.menu.menus = vec![
            MenuDefinition {
                id: "root".into(),
                title: "Root".into(),
                extensions: Vec::new(),
                context_default: false,
                actions: vec![
                    MenuActionConfig {
                        id: "common".into(),
                        label: "Common".into(),
                        icon: None,
                        default: None,
                        submenu: Some("common".into()),
                        command: None,
                    },
                    MenuActionConfig {
                        id: "editor".into(),
                        label: "Editor".into(),
                        icon: None,
                        default: None,
                        submenu: Some("editor".into()),
                        command: None,
                    },
                ],
            },
            MenuDefinition {
                id: "common".into(),
                title: "Common".into(),
                extensions: Vec::new(),
                context_default: false,
                actions: vec![MenuActionConfig {
                    id: "apps".into(),
                    label: "Apps".into(),
                    icon: None,
                    default: None,
                    submenu: None,
                    command: Some(MenuActionCommandConfig {
                        kind: "quick-path".into(),
                        provider: Some("~/Apps".into()),
                        desktop_id: None,
                    }),
                }],
            },
            MenuDefinition {
                id: "editor".into(),
                title: "Editor".into(),
                extensions: vec!["toml".into()],
                context_default: true,
                actions: vec![MenuActionConfig {
                    id: "kwrite".into(),
                    label: "Kwrite".into(),
                    icon: None,
                    default: None,
                    submenu: None,
                    command: Some(MenuActionCommandConfig {
                        kind: "shell".into(),
                        provider: Some("kwrite %f".into()),
                        desktop_id: None,
                    }),
                }],
            },
        ];

        let context = Context {
            source: ContextSource::ExplicitFiles,
            kind: ContextKind::File,
            raw: "/tmp/Cargo.toml".into(),
            text: None,
            files: vec![PathBuf::from("/tmp/Cargo.toml")],
            mime_types: vec!["text/plain".into()],
        };

        let menu = build_model(&config, "root", &context).unwrap();

        assert_eq!(menu.actions.len(), 1);
        assert_eq!(menu.actions[0].id, "common");
    }

    fn text_context() -> Context {
        Context {
            source: ContextSource::ExplicitText,
            kind: ContextKind::Text,
            raw: "hello world".into(),
            text: Some("hello world".into()),
            files: Vec::new(),
            mime_types: Vec::new(),
        }
    }

    fn sample_loaded_config() -> LoadedConfig {
        let mut providers = BTreeMap::new();
        providers.insert(
            "duckduckgo".into(),
            crate::config::ProviderConfig {
                label: "DuckDuckGo".into(),
                url: "https://duckduckgo.com/?q={query}".into(),
                default: Some(true),
            },
        );

        LoadedConfig {
            app: AppConfig {
                general: None,
                ui: None,
                actions: None,
                menus: Some(MenusConfig {
                    default_file: None,
                    default_id: None,
                    fallback_id: None,
                }),
                providers,
                rules: Vec::new(),
            },
            menu: MenuConfig {
                menus: vec![MenuDefinition {
                    id: "text".into(),
                    title: "Text".into(),
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
                }],
            },
            sources: ResolvedConfigPaths {
                config_toml: PathBuf::from("config.toml"),
                menu_file: PathBuf::from("menu.ini"),
            },
        }
    }

    fn submenu_loaded_config() -> LoadedConfig {
        let mut config = sample_loaded_config();
        config.app.providers.insert(
            "github".into(),
            crate::config::ProviderConfig {
                label: "GitHub".into(),
                url: "https://github.com/search?q={query}".into(),
                default: None,
            },
        );
        config.menu.menus = vec![
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
                title: "Search providers".into(),
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
        ];
        config
    }
}
