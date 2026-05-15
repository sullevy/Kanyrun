use crate::config::{LoadedConfig, RuleConfig, RuleMatch};
use crate::context::{Context, ContextKind, ContextSource};
use crate::menu::ActionCommand;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Direct(ActionCommand),
    Menu(crate::menu::MenuModel),
    None,
}

pub fn resolve(
    config: &LoadedConfig,
    context: &Context,
    force_menu: bool,
) -> Result<Resolution, String> {
    if context.files.len() > 1
        && let Some(menu_id) = menu_id(config, "file-list")
    {
        return resolve_menu(config, context, menu_id);
    }

    if matches!(context.kind, ContextKind::File | ContextKind::Path)
        && let Some(menu_id) = extension_context_default_menu_id(config, context)
    {
        return resolve_menu(config, context, menu_id);
    }

    if force_menu
        && let Some(menu_id) =
            context_default_menu_id(config, context).or_else(|| default_menu_id(config))
    {
        return resolve_menu(config, context, menu_id);
    }

    for rule in &config.app.rules {
        if !matches_rule(&rule.when, context) {
            continue;
        }

        return match rule.behavior.as_str() {
            "direct" if !force_menu => {
                let action = rule
                    .action
                    .as_deref()
                    .ok_or_else(|| "direct rule missing action".to_string())?;
                Ok(Resolution::Direct(parse_direct_action(action)?))
            }
            "menu" | "direct" => {
                let menu_id = rule
                    .menu
                    .as_deref()
                    .or_else(|| fallback_menu_id_for_direct(rule, force_menu))
                    .ok_or_else(|| format!("rule for {:?} missing menu", context.kind))?;
                resolve_menu(config, context, menu_id)
            }
            "none" => Ok(Resolution::None),
            other => Err(format!("unsupported rule behavior: {other}")),
        };
    }

    match fallback_menu_id(config) {
        Some(menu_id) => resolve_menu(config, context, menu_id),
        None => Ok(Resolution::None),
    }
}

fn fallback_menu_id_for_direct<'a>(rule: &'a RuleConfig, force_menu: bool) -> Option<&'a str> {
    if force_menu {
        rule.menu.as_deref()
    } else {
        None
    }
}

fn resolve_menu(
    config: &LoadedConfig,
    context: &Context,
    menu_id: &str,
) -> Result<Resolution, String> {
    let menu = crate::menu::build_model(config, menu_id, context)?;
    if !menu.actions.is_empty() {
        return Ok(Resolution::Menu(menu));
    }

    let Some(fallback_id) = fallback_menu_id(config) else {
        return Ok(Resolution::None);
    };
    if fallback_id == menu_id {
        return Ok(Resolution::None);
    }

    let fallback = crate::menu::build_model(config, fallback_id, context)?;
    if fallback.actions.is_empty() {
        Ok(Resolution::None)
    } else {
        Ok(Resolution::Menu(fallback))
    }
}

fn fallback_menu_id<'a>(config: &'a LoadedConfig) -> Option<&'a str> {
    config
        .app
        .menus
        .as_ref()
        .and_then(|menus| menus.fallback_id.as_deref())
        .or_else(|| default_menu_id(config))
}

fn default_menu_id(config: &LoadedConfig) -> Option<&str> {
    config
        .app
        .menus
        .as_ref()
        .and_then(|menus| menus.default_id.as_deref())
        .or_else(|| config.menu.menus.first().map(|menu| menu.id.as_str()))
}

fn context_default_menu_id<'a>(config: &'a LoadedConfig, context: &Context) -> Option<&'a str> {
    if context.files.len() > 1 {
        return menu_id(config, "file-list");
    }

    match context.kind {
        ContextKind::FileList => menu_id(config, "file-list"),
        ContextKind::Directory => menu_id(config, "directory"),
        ContextKind::Text | ContextKind::Url => None,
        ContextKind::File | ContextKind::Path => {
            extension_context_default_menu_id(config, context).or_else(|| menu_id(config, "file"))
        }
        ContextKind::Empty => None,
    }
}

fn extension_context_default_menu_id<'a>(
    config: &'a LoadedConfig,
    context: &Context,
) -> Option<&'a str> {
    config
        .menu
        .menus
        .iter()
        .find(|menu| {
            menu.context_default
                && !menu.extensions.is_empty()
                && menu_matches_extensions(menu, context)
        })
        .map(|menu| menu.id.as_str())
}

fn menu_id<'a>(config: &'a LoadedConfig, id: &str) -> Option<&'a str> {
    config
        .menu
        .menus
        .iter()
        .find(|menu| menu.id == id)
        .map(|menu| menu.id.as_str())
}

fn menu_matches_extensions(menu: &crate::config::MenuDefinition, context: &Context) -> bool {
    context.files.iter().any(|path| {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                menu.extensions
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(ext))
            })
            .unwrap_or(false)
    })
}

fn matches_rule(rule: &RuleMatch, context: &Context) -> bool {
    matches_kind(rule, context)
        && matches_source(rule, context)
        && matches_mime(rule, context)
        && matches_mime_glob(rule, context)
        && matches_extensions(rule, context)
        && matches_is_multi(rule, context)
}

fn matches_kind(rule: &RuleMatch, context: &Context) -> bool {
    match rule.kind.as_deref() {
        Some(kind) => kind == kind_name(context.kind),
        None => true,
    }
}

fn matches_source(rule: &RuleMatch, context: &Context) -> bool {
    match rule.source.as_deref() {
        Some(source) => source == source_name(context.source),
        None => true,
    }
}

fn matches_mime(rule: &RuleMatch, context: &Context) -> bool {
    match rule.mime.as_deref() {
        Some(expected) => context.mime_types.iter().any(|mime| mime == expected),
        None => true,
    }
}

fn matches_mime_glob(rule: &RuleMatch, context: &Context) -> bool {
    match rule.mime_glob.as_deref() {
        Some(pattern) if pattern.ends_with("/*") => {
            let prefix = &pattern[..pattern.len() - 1];
            context
                .mime_types
                .iter()
                .any(|mime| mime.starts_with(prefix))
        }
        Some(pattern) => context.mime_types.iter().any(|mime| mime == pattern),
        None => true,
    }
}

fn matches_extensions(rule: &RuleMatch, context: &Context) -> bool {
    match rule.extensions.as_ref() {
        Some(extensions) => context.files.iter().any(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| {
                    extensions
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(ext))
                })
                .unwrap_or(false)
        }),
        None => true,
    }
}

fn matches_is_multi(rule: &RuleMatch, context: &Context) -> bool {
    match rule.is_multi {
        Some(expected) => expected == (context.files.len() > 1),
        None => true,
    }
}

fn parse_direct_action(action: &str) -> Result<ActionCommand, String> {
    match action {
        "open-url" => Ok(ActionCommand::OpenUrl),
        "open-file" => Ok(ActionCommand::OpenFile),
        "open-directory" => Ok(ActionCommand::OpenDirectory),
        "reveal-file" => Ok(ActionCommand::RevealFile),
        "copy-text" => Ok(ActionCommand::CopyText),
        "copy-path" => Ok(ActionCommand::CopyPath),
        other => Err(format!("unsupported direct action: {other}")),
    }
}

fn kind_name(kind: ContextKind) -> &'static str {
    match kind {
        ContextKind::Url => "url",
        ContextKind::Text => "text",
        ContextKind::Path => "path",
        ContextKind::File => "file",
        ContextKind::Directory => "directory",
        ContextKind::FileList => "file-list",
        ContextKind::Empty => "empty",
    }
}

fn source_name(source: ContextSource) -> &'static str {
    match source {
        ContextSource::ExplicitText => "explicit-text",
        ContextSource::ExplicitFiles => "explicit-files",
        ContextSource::Clipboard => "clipboard",
        ContextSource::PrimarySelection => "primary-selection",
        ContextSource::Empty => "empty",
    }
}

#[cfg(test)]
mod tests {
    use super::{Resolution, resolve};
    use crate::config::{
        AppConfig, LoadedConfig, MenuConfig, MenuDefinition, ProviderConfig, ResolvedConfigPaths,
        RuleConfig, RuleMatch, config_paths_from, load,
    };
    use crate::context::{Context, ContextKind, ContextSource};
    use crate::menu::ActionCommand;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn resolves_direct_url_rule() {
        let config = sample_loaded_config();
        let context = Context {
            source: ContextSource::ExplicitText,
            kind: ContextKind::Url,
            raw: "https://example.com".into(),
            text: Some("https://example.com".into()),
            files: Vec::new(),
            mime_types: Vec::new(),
        };

        let resolution = resolve(&config, &context, false).unwrap();

        assert_eq!(resolution, Resolution::Direct(ActionCommand::OpenUrl));
    }

    #[test]
    fn resolves_mime_specific_file_menu_before_generic_file_rule() {
        let config = sample_loaded_config();
        let context = Context {
            source: ContextSource::ExplicitFiles,
            kind: ContextKind::File,
            raw: "/tmp/doc.pdf".into(),
            text: None,
            files: vec![PathBuf::from("/tmp/doc.pdf")],
            mime_types: vec!["application/pdf".into()],
        };

        let resolution = resolve(&config, &context, false).unwrap();

        match resolution {
            Resolution::Menu(menu) => assert_eq!(menu.id, "pdf"),
            other => panic!("expected menu resolution, got {other:?}"),
        }
    }

    #[test]
    fn resolves_image_menu_from_mime_glob() {
        let config = sample_loaded_config();
        let context = Context {
            source: ContextSource::ExplicitFiles,
            kind: ContextKind::File,
            raw: "/tmp/photo.png".into(),
            text: None,
            files: vec![PathBuf::from("/tmp/photo.png")],
            mime_types: vec!["image/png".into()],
        };

        let resolution = resolve(&config, &context, false).unwrap();

        match resolution {
            Resolution::Menu(menu) => assert_eq!(menu.id, "image"),
            other => panic!("expected menu resolution, got {other:?}"),
        }
    }

    #[test]
    fn resolves_directory_menu() {
        let config = sample_loaded_config();
        let context = Context {
            source: ContextSource::ExplicitFiles,
            kind: ContextKind::Directory,
            raw: "/tmp/workspace".into(),
            text: None,
            files: vec![PathBuf::from("/tmp/workspace")],
            mime_types: Vec::new(),
        };

        let resolution = resolve(&config, &context, false).unwrap();

        match resolution {
            Resolution::Menu(menu) => assert_eq!(menu.id, "directory"),
            other => panic!("expected menu resolution, got {other:?}"),
        }
    }

    #[test]
    fn resolves_file_list_menu() {
        let config = sample_loaded_config();
        let context = Context {
            source: ContextSource::ExplicitFiles,
            kind: ContextKind::FileList,
            raw: "/tmp/a.md\n/tmp/b.png".into(),
            text: None,
            files: vec![PathBuf::from("/tmp/a.md"), PathBuf::from("/tmp/b.png")],
            mime_types: vec!["text/markdown".into(), "image/png".into()],
        };

        let resolution = resolve(&config, &context, false).unwrap();

        match resolution {
            Resolution::Menu(menu) => assert_eq!(menu.id, "file-list"),
            other => panic!("expected menu resolution, got {other:?}"),
        }
    }

    #[test]
    fn multiple_paths_use_file_list_even_if_kind_is_file() {
        let config = sample_loaded_config();
        let context = Context {
            source: ContextSource::ExplicitFiles,
            kind: ContextKind::File,
            raw: "/tmp/a.md\n/tmp/b.png".into(),
            text: None,
            files: vec![PathBuf::from("/tmp/a.md"), PathBuf::from("/tmp/b.png")],
            mime_types: vec!["text/markdown".into(), "image/png".into()],
        };

        let resolution = resolve(&config, &context, false).unwrap();

        match resolution {
            Resolution::Menu(menu) => assert_eq!(menu.id, "file-list"),
            other => panic!("expected menu resolution, got {other:?}"),
        }
    }

    #[test]
    fn resolves_text_file_before_generic_file_menu() {
        let config = sample_loaded_config();
        let context = Context {
            source: ContextSource::ExplicitFiles,
            kind: ContextKind::File,
            raw: "/tmp/notes.txt".into(),
            text: None,
            files: vec![PathBuf::from("/tmp/notes.txt")],
            mime_types: vec!["text/plain".into()],
        };

        let resolution = resolve(&config, &context, false).unwrap();

        match resolution {
            Resolution::Menu(menu) => assert_eq!(menu.id, "text-file"),
            other => panic!("expected menu resolution, got {other:?}"),
        }
    }

    #[test]
    fn falls_back_to_configured_menu_when_selected_menu_filters_out() {
        let mut config = sample_loaded_config();
        config.app.menus = Some(crate::config::MenusConfig {
            default_file: None,
            default_id: Some("root".into()),
            fallback_id: Some("fallback".into()),
        });
        config.menu.menus = vec![
            MenuDefinition {
                id: "root".into(),
                title: "Root".into(),
                extensions: Vec::new(),
                context_default: false,
                actions: Vec::new(),
            },
            MenuDefinition {
                id: "fallback".into(),
                title: "Fallback".into(),
                extensions: Vec::new(),
                context_default: false,
                actions: vec![crate::config::MenuActionConfig {
                    id: "copy-path".into(),
                    label: "Copy path".into(),
                    icon: None,
                    default: Some(true),
                    submenu: None,
                    command: Some(crate::config::MenuActionCommandConfig {
                        kind: "copy-path".into(),
                        provider: None,
                        desktop_id: None,
                    }),
                }],
            },
            MenuDefinition {
                id: "file".into(),
                title: "File".into(),
                context_default: false,
                extensions: vec!["py".into()],
                actions: vec![crate::config::MenuActionConfig {
                    id: "open-file".into(),
                    label: "Open".into(),
                    icon: None,
                    default: Some(true),
                    submenu: None,
                    command: Some(crate::config::MenuActionCommandConfig {
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
            mime_types: vec!["application/octet-stream".into()],
        };

        match resolve(&config, &context, false).unwrap() {
            Resolution::Menu(menu) => assert_eq!(menu.id, "fallback"),
            other => panic!("expected fallback menu, got {other:?}"),
        }
    }

    #[test]
    fn bundled_config_resolves_common_contexts() {
        let temp = TempDir::new().unwrap();
        let repo_root = temp.path().join("repo");
        let bundled = repo_root.join("assets/config");
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::create_dir_all(temp.path().join("xdg")).unwrap();
        std::fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/config/config.toml"),
            bundled.join("config.toml"),
        )
        .unwrap();
        std::fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/config/menu.toml"),
            bundled.join("menu.toml"),
        )
        .unwrap();

        let loaded = load(
            &config_paths_from(Some(temp.path().join("xdg")), None, repo_root),
            None,
        )
        .unwrap();

        assert_eq!(
            resolve(
                &loaded,
                &Context {
                    source: ContextSource::ExplicitText,
                    kind: ContextKind::Url,
                    raw: "https://example.com".into(),
                    text: Some("https://example.com".into()),
                    files: Vec::new(),
                    mime_types: Vec::new(),
                },
                false,
            )
            .unwrap(),
            Resolution::Direct(ActionCommand::OpenUrl)
        );

        assert_menu_id(
            &loaded,
            Context {
                source: ContextSource::ExplicitText,
                kind: ContextKind::Text,
                raw: "hello world".into(),
                text: Some("hello world".into()),
                files: Vec::new(),
                mime_types: Vec::new(),
            },
            "root",
        );
        assert_menu_id(
            &loaded,
            Context {
                source: ContextSource::ExplicitFiles,
                kind: ContextKind::File,
                raw: "/tmp/doc.pdf".into(),
                text: None,
                files: vec![PathBuf::from("/tmp/doc.pdf")],
                mime_types: vec!["application/pdf".into()],
            },
            "pdf",
        );
        assert_menu_id(
            &loaded,
            Context {
                source: ContextSource::ExplicitFiles,
                kind: ContextKind::File,
                raw: "/tmp/photo.png".into(),
                text: None,
                files: vec![PathBuf::from("/tmp/photo.png")],
                mime_types: vec!["image/png".into()],
            },
            "image",
        );
        assert_menu_id(
            &loaded,
            Context {
                source: ContextSource::ExplicitFiles,
                kind: ContextKind::File,
                raw: "/tmp/notes.txt".into(),
                text: None,
                files: vec![PathBuf::from("/tmp/notes.txt")],
                mime_types: vec!["text/plain".into()],
            },
            "text-file",
        );
        assert_menu_id(
            &loaded,
            Context {
                source: ContextSource::ExplicitFiles,
                kind: ContextKind::Directory,
                raw: "/tmp/workspace".into(),
                text: None,
                files: vec![PathBuf::from("/tmp/workspace")],
                mime_types: Vec::new(),
            },
            "directory",
        );
        assert_menu_id(
            &loaded,
            Context {
                source: ContextSource::ExplicitFiles,
                kind: ContextKind::FileList,
                raw: "/tmp/a.txt\n/tmp/b.txt".into(),
                text: None,
                files: vec![PathBuf::from("/tmp/a.txt"), PathBuf::from("/tmp/b.txt")],
                mime_types: vec!["text/plain".into(), "text/plain".into()],
            },
            "file-list",
        );
        assert_menu_id(
            &loaded,
            Context {
                source: ContextSource::ExplicitFiles,
                kind: ContextKind::File,
                raw: "/tmp/archive.bin".into(),
                text: None,
                files: vec![PathBuf::from("/tmp/archive.bin")],
                mime_types: vec!["application/octet-stream".into()],
            },
            "file",
        );
    }

    #[test]
    fn forced_menu_uses_context_default_ids() {
        let mut config = sample_loaded_config();
        config.app.rules = Vec::new();
        config.menu.menus.iter_mut().for_each(|menu| {
            menu.context_default = matches!(menu.id.as_str(), "directory" | "file-list" | "file");
        });
        config.menu.menus.push(MenuDefinition {
            id: "editor".into(),
            title: "Editor".into(),
            extensions: vec!["toml".into()],
            context_default: true,
            actions: vec![dummy_action("edit", "open-file")],
        });
        assert_forced_menu_id(
            &config,
            Context {
                source: ContextSource::ExplicitFiles,
                kind: ContextKind::File,
                raw: "/tmp/Cargo.toml".into(),
                text: None,
                files: vec![PathBuf::from("/tmp/Cargo.toml")],
                mime_types: vec!["text/plain".into()],
            },
            "editor",
        );
        assert_forced_menu_id(
            &config,
            Context {
                source: ContextSource::ExplicitFiles,
                kind: ContextKind::File,
                raw: "/tmp/archive.bin".into(),
                text: None,
                files: vec![PathBuf::from("/tmp/archive.bin")],
                mime_types: vec!["application/octet-stream".into()],
            },
            "file",
        );
        assert_forced_menu_id(
            &config,
            Context {
                source: ContextSource::ExplicitText,
                kind: ContextKind::Text,
                raw: "hello".into(),
                text: Some("hello".into()),
                files: Vec::new(),
                mime_types: Vec::new(),
            },
            "root",
        );
    }

    #[test]
    fn file_extension_context_default_takes_precedence_over_generic_file_rule() {
        let mut config = sample_loaded_config();
        config.menu.menus.push(MenuDefinition {
            id: "office".into(),
            title: "Office".into(),
            extensions: vec!["doc".into(), "docx".into(), "xlsx".into()],
            context_default: true,
            actions: vec![dummy_action("wps", "open-file")],
        });

        assert_menu_id(
            &config,
            Context {
                source: ContextSource::ExplicitFiles,
                kind: ContextKind::File,
                raw: "/tmp/report.docx".into(),
                text: None,
                files: vec![PathBuf::from("/tmp/report.docx")],
                mime_types: vec!["application/octet-stream".into()],
            },
            "office",
        );
    }

    #[test]
    fn empty_context_uses_default_menu() {
        let config = sample_loaded_config();

        assert_menu_id(&config, Context::empty(ContextSource::Empty), "root");
    }

    fn assert_menu_id(config: &LoadedConfig, context: Context, expected_menu_id: &str) {
        match resolve(config, &context, false).unwrap() {
            Resolution::Menu(menu) => assert_eq!(menu.id, expected_menu_id),
            other => panic!("expected menu resolution, got {other:?}"),
        }
    }

    fn assert_forced_menu_id(config: &LoadedConfig, context: Context, expected_menu_id: &str) {
        match resolve(config, &context, true).unwrap() {
            Resolution::Menu(menu) => assert_eq!(menu.id, expected_menu_id),
            other => panic!("expected menu resolution, got {other:?}"),
        }
    }

    fn sample_loaded_config() -> LoadedConfig {
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
                general: None,
                ui: None,
                actions: None,
                menus: Some(crate::config::MenusConfig {
                    default_file: None,
                    default_id: Some("root".into()),
                    fallback_id: None,
                }),
                providers,
                rules: vec![
                    RuleConfig {
                        when: RuleMatch {
                            kind: Some("file".into()),
                            source: None,
                            mime: Some("application/pdf".into()),
                            mime_glob: None,
                            extensions: None,
                            is_multi: None,
                        },
                        behavior: "menu".into(),
                        action: None,
                        menu: Some("pdf".into()),
                    },
                    RuleConfig {
                        when: RuleMatch {
                            kind: Some("file".into()),
                            source: None,
                            mime: None,
                            mime_glob: Some("image/*".into()),
                            extensions: None,
                            is_multi: None,
                        },
                        behavior: "menu".into(),
                        action: None,
                        menu: Some("image".into()),
                    },
                    RuleConfig {
                        when: RuleMatch {
                            kind: Some("file".into()),
                            source: None,
                            mime: None,
                            mime_glob: Some("text/*".into()),
                            extensions: None,
                            is_multi: None,
                        },
                        behavior: "menu".into(),
                        action: None,
                        menu: Some("text-file".into()),
                    },
                    RuleConfig {
                        when: RuleMatch {
                            kind: Some("directory".into()),
                            source: None,
                            mime: None,
                            mime_glob: None,
                            extensions: None,
                            is_multi: None,
                        },
                        behavior: "menu".into(),
                        action: None,
                        menu: Some("directory".into()),
                    },
                    RuleConfig {
                        when: RuleMatch {
                            kind: Some("file-list".into()),
                            source: None,
                            mime: None,
                            mime_glob: None,
                            extensions: None,
                            is_multi: None,
                        },
                        behavior: "menu".into(),
                        action: None,
                        menu: Some("file-list".into()),
                    },
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
                            kind: Some("file".into()),
                            source: None,
                            mime: None,
                            mime_glob: None,
                            extensions: None,
                            is_multi: None,
                        },
                        behavior: "menu".into(),
                        action: None,
                        menu: Some("file".into()),
                    },
                ],
            },
            menu: MenuConfig {
                menus: vec![
                    MenuDefinition {
                        id: "root".into(),
                        title: "Root".into(),
                        extensions: Vec::new(),
                        context_default: false,
                        actions: vec![
                            dummy_action("google", "quick-url"),
                            crate::config::MenuActionConfig {
                                id: "common".into(),
                                label: "common".into(),
                                icon: None,
                                default: None,
                                submenu: Some("common".into()),
                                command: None,
                            },
                            crate::config::MenuActionConfig {
                                id: "image-submenu".into(),
                                label: "image".into(),
                                icon: None,
                                default: None,
                                submenu: Some("image".into()),
                                command: None,
                            },
                        ],
                    },
                    MenuDefinition {
                        id: "common".into(),
                        title: "Common".into(),
                        extensions: Vec::new(),
                        context_default: false,
                        actions: vec![dummy_action("apps", "quick-path")],
                    },
                    MenuDefinition {
                        id: "file".into(),
                        title: "File".into(),
                        extensions: Vec::new(),
                        context_default: false,
                        actions: vec![dummy_action("open-file", "open-file")],
                    },
                    MenuDefinition {
                        id: "pdf".into(),
                        title: "PDF".into(),
                        extensions: Vec::new(),
                        context_default: false,
                        actions: vec![dummy_action("open-pdf", "open-file")],
                    },
                    MenuDefinition {
                        id: "image".into(),
                        title: "Image".into(),
                        extensions: Vec::new(),
                        context_default: false,
                        actions: vec![dummy_action("open-image", "open-file")],
                    },
                    MenuDefinition {
                        id: "text-file".into(),
                        title: "Text file".into(),
                        extensions: Vec::new(),
                        context_default: false,
                        actions: vec![dummy_action("open-text", "open-file")],
                    },
                    MenuDefinition {
                        id: "directory".into(),
                        title: "Directory".into(),
                        extensions: Vec::new(),
                        context_default: false,
                        actions: vec![dummy_action("open-directory", "open-directory")],
                    },
                    MenuDefinition {
                        id: "file-list".into(),
                        title: "Files".into(),
                        extensions: Vec::new(),
                        context_default: false,
                        actions: vec![dummy_action("copy-path", "copy-path")],
                    },
                ],
            },
            sources: ResolvedConfigPaths {
                config_toml: PathBuf::from("config.toml"),
                menu_file: PathBuf::from("menu.ini"),
            },
        }
    }

    fn dummy_action(id: &str, kind: &str) -> crate::config::MenuActionConfig {
        let provider = match kind {
            "quick-url" => Some("https://example.com?q=%s".into()),
            "quick-path" => Some("~/Apps".into()),
            _ => None,
        };

        crate::config::MenuActionConfig {
            id: id.into(),
            label: id.into(),
            icon: None,
            default: Some(true),
            submenu: None,
            command: Some(crate::config::MenuActionCommandConfig {
                kind: kind.into(),
                provider,
                desktop_id: None,
            }),
        }
    }
}
