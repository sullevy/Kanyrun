use chrono::Local;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ConfigPaths {
    pub user_dir: PathBuf,
    pub bundled_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedConfigPaths {
    pub config_toml: PathBuf,
    pub menu_file: PathBuf,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AppConfig {
    pub general: Option<GeneralConfig>,
    pub ui: Option<UiConfig>,
    pub actions: Option<ActionsConfig>,
    pub menus: Option<MenusConfig>,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub rules: Vec<RuleConfig>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GeneralConfig {
    pub file_manager: Option<String>,
    pub terminal: Option<String>,
    pub default_browser_opener: Option<String>,
    pub default_mode: Option<String>,
    pub no_context_behavior: Option<String>,
    pub prefer_primary_selection: Option<bool>,
    pub fallback_to_clipboard: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct UiConfig {
    pub frontend: Option<String>,
    pub close_on_outside_click: Option<bool>,
    pub close_on_escape: Option<bool>,
    pub show_icons: Option<bool>,
    pub show_context_header: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ActionsConfig {
    pub reveal_command: Option<Vec<String>>,
    pub open_directory_command: Option<Vec<String>>,
    pub copy_command: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct MenusConfig {
    pub default_file: Option<String>,
    pub default_id: Option<String>,
    pub fallback_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ProviderConfig {
    pub label: String,
    pub url: String,
    pub default: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RuleConfig {
    pub when: RuleMatch,
    pub behavior: String,
    pub action: Option<String>,
    pub menu: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RuleMatch {
    pub kind: Option<String>,
    pub source: Option<String>,
    pub mime: Option<String>,
    pub mime_glob: Option<String>,
    pub extensions: Option<Vec<String>>,
    pub is_multi: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct MenuConfig {
    pub menus: Vec<MenuDefinition>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct MenuDefinition {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub context_default: bool,
    pub actions: Vec<MenuActionConfig>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct MenuActionConfig {
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    pub default: Option<bool>,
    pub submenu: Option<String>,
    pub command: Option<MenuActionCommandConfig>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct MenuActionCommandConfig {
    #[serde(rename = "type")]
    pub kind: String,
    pub provider: Option<String>,
    pub desktop_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedConfig {
    pub app: AppConfig,
    pub menu: MenuConfig,
    pub sources: ResolvedConfigPaths,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedMenuLineKind {
    Item,
    Category1,
    Category2,
    Separator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedMenuLine {
    kind: ParsedMenuLineKind,
    content: String,
    indent: usize,
}

pub fn config_paths() -> ConfigPaths {
    let user_base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|value| PathBuf::from(value).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    ConfigPaths {
        user_dir: user_base.join("kanyrun"),
        bundled_dir: bundled_config_dir_for(std::env::current_exe().ok().as_deref(), &repo_root),
    }
}

fn bundled_config_dir_for(current_exe: Option<&Path>, repo_root: &Path) -> PathBuf {
    if let Some(path) = current_exe
        .and_then(|current| current.parent().map(|dir| dir.join("config")))
        .filter(|candidate| candidate.is_dir())
    {
        return path;
    }

    repo_root.join("assets").join("config")
}

#[cfg(test)]
pub fn config_paths_from(
    xdg_config_home: Option<PathBuf>,
    home: Option<PathBuf>,
    repo_root: PathBuf,
) -> ConfigPaths {
    let user_base = xdg_config_home
        .or_else(|| home.map(|value| value.join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));

    ConfigPaths {
        user_dir: user_base.join("kanyrun"),
        bundled_dir: repo_root.join("assets").join("config"),
    }
}

pub fn load(paths: &ConfigPaths, menu_override: Option<&Path>) -> Result<LoadedConfig, String> {
    let config_toml = prefer_existing(paths.user_dir.join("config.toml"), paths.bundled_dir.join("config.toml"));
    let app: AppConfig = read_toml(&config_toml)?;
    let mut sources = resolve_config_sources(paths, app.menus.as_ref(), menu_override);
    let menu = match read_menu_config(&sources.menu_file) {
        Ok(menu) => menu,
        Err(_) if menu_override.is_none() && app.menus.as_ref().and_then(|menus| menus.default_file.as_deref()).is_some() => {
            sources = resolve_config_sources(paths, None, None);
            read_menu_config(&sources.menu_file)?
        }
        Err(error) => return Err(error),
    };

    Ok(LoadedConfig { app, menu, sources })
}

pub fn resolve_config_sources(
    paths: &ConfigPaths,
    menus: Option<&MenusConfig>,
    menu_override: Option<&Path>,
) -> ResolvedConfigPaths {
    ResolvedConfigPaths {
        config_toml: prefer_existing(paths.user_dir.join("config.toml"), paths.bundled_dir.join("config.toml")),
        menu_file: resolve_menu_file(paths, menus, menu_override),
    }
}

fn resolve_menu_file(paths: &ConfigPaths, menus: Option<&MenusConfig>, menu_override: Option<&Path>) -> PathBuf {
    if let Some(path) = menu_override {
        return path.to_path_buf();
    }

    if let Some(default_file) = menus.and_then(|menus| menus.default_file.as_deref()) {
        return expand_menu_path(default_file, paths);
    }

    let user_ini = paths.user_dir.join("menu.ini");
    if user_ini.is_file() {
        return user_ini;
    }

    let user_conf = paths.user_dir.join("menu.conf");
    if user_conf.is_file() {
        return user_conf;
    }

    let bundled_ini = paths.bundled_dir.join("menu.ini");
    if bundled_ini.is_file() {
        return bundled_ini;
    }

    let bundled_conf = paths.bundled_dir.join("menu.conf");
    if bundled_conf.is_file() {
        return bundled_conf;
    }

    let bundled_toml = paths.bundled_dir.join("menu.toml");
    if bundled_toml.is_file() {
        return bundled_toml;
    }

    paths.user_dir.join("menu.ini")
}

fn expand_menu_path(raw: &str, paths: &ConfigPaths) -> PathBuf {
    let expanded = raw.trim();
    if expanded.is_empty() {
        return paths.user_dir.join("menu.ini");
    }

    let path = if let Some(home_relative) = expanded.strip_prefix("~/") {
        home_dir()
            .map(|home| home.join(home_relative))
            .unwrap_or_else(|| PathBuf::from(expanded))
    } else {
        PathBuf::from(expanded)
    };

    if path.is_absolute() {
        return path;
    }

    let user_relative = paths.user_dir.join(&path);
    if user_relative.is_file() {
        return user_relative;
    }

    let bundled_relative = paths.bundled_dir.join(&path);
    if bundled_relative.is_file() {
        return bundled_relative;
    }

    user_relative
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn prefer_existing(user_path: PathBuf, bundled_path: PathBuf) -> PathBuf {
    if user_path.is_file() {
        user_path
    } else {
        bundled_path
    }
}

fn read_toml<T>(path: &Path) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    toml::from_str(&raw).map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn read_menu_config(path: &Path) -> Result<MenuConfig, String> {
    match path.extension().and_then(|value| value.to_str()) {
        Some("toml") => read_toml(path),
        _ => read_ini_menu(path),
    }
}

fn read_ini_menu(path: &Path) -> Result<MenuConfig, String> {
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    parse_ini_menu(&raw)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn parse_ini_menu(raw: &str) -> Result<MenuConfig, String> {
    let lines = parse_menu_lines(raw);
    let mut root_actions = Vec::new();
    let mut menus = Vec::new();
    let mut level1_index = None;
    let mut level1_child_indent = None;
    let mut level2_index = None;
    let mut level2_child_indent = None;
    let mut submenu_counter = 0usize;
    let mut action_counter = 0usize;

    for line in lines {
        match line.kind {
            ParsedMenuLineKind::Category1 => {
                if line.indent != 0 {
                    return Err(format!("top-level submenu must not be indented: {}", line.content));
                }

                let (label, extensions, explicit_id, context_default) = parse_submenu_header(&line.content);
                let submenu_id = explicit_id.unwrap_or_else(|| next_submenu_id(&mut submenu_counter));
                menus.push(MenuDefinition {
                    id: submenu_id.clone(),
                    title: label.to_string(),
                    extensions,
                    context_default,
                    actions: Vec::new(),
                });
                level1_index = Some(menus.len() - 1);
                level1_child_indent = None;
                level2_index = None;
                level2_child_indent = None;
                root_actions.push(submenu_action(&label, &submenu_id, &mut action_counter));
            }
            ParsedMenuLineKind::Category2 => {
                let Some(parent_index) = level1_index else {
                    return Err("second-level submenu requires a parent submenu".into());
                };
                if line.indent == 0 {
                    return Err(format!("second-level submenu must be indented: {}", line.content));
                }

                match level1_child_indent {
                    Some(indent) if line.indent != indent => {
                        return Err(format!("second-level submenu must align with submenu children: {}", line.content));
                    }
                    Some(_) => {}
                    None => level1_child_indent = Some(line.indent),
                }

                let (label, extensions, explicit_id, context_default) = parse_submenu_header(&line.content);
                let submenu_id = explicit_id.unwrap_or_else(|| next_submenu_id(&mut submenu_counter));
                menus.push(MenuDefinition {
                    id: submenu_id.clone(),
                    title: label.clone(),
                    extensions,
                    context_default,
                    actions: Vec::new(),
                });
                let child_index = menus.len() - 1;
                menus[parent_index]
                    .actions
                    .push(submenu_action(&label, &submenu_id, &mut action_counter));
                level2_index = Some(child_index);
                level2_child_indent = None;
            }
            ParsedMenuLineKind::Separator => {
                let target = actions_for_indented_line(
                    &line,
                    &mut menus,
                    level1_index,
                    &mut level1_child_indent,
                    &mut level2_index,
                    &mut level2_child_indent,
                    "separator must be indented inside a submenu",
                )?;
                target.push(separator_action(&mut action_counter));
            }
            ParsedMenuLineKind::Item => {
                let target = if line.indent == 0 {
                    if level1_index.is_some() {
                        return Err(format!("submenu child must be indented: {}", line.content));
                    }
                    &mut root_actions
                } else {
                    actions_for_indented_line(
                        &line,
                        &mut menus,
                        level1_index,
                        &mut level1_child_indent,
                        &mut level2_index,
                        &mut level2_child_indent,
                        &format!("indented item requires a parent submenu: {}", line.content),
                    )?
                };

                let (label, command) = split_item_line(&line.content);
                let (label, is_default) = parse_default_marker(&label);
                target.push(MenuActionConfig {
                    id: sanitize_id(label, "action", &mut action_counter),
                    label: label.to_string(),
                    icon: None,
                    default: is_default.then_some(true),
                    submenu: None,
                    command: Some(parse_command(&command)?),
                });
            }
        }
    }

    menus.insert(
        0,
        MenuDefinition {
            id: "root".into(),
            title: "Menu".into(),
            extensions: Vec::new(),
            context_default: false,
            actions: root_actions,
        },
    );

    Ok(MenuConfig { menus })
}

fn parse_menu_lines(raw: &str) -> Vec<ParsedMenuLine> {
    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(';') {
                return None;
            }

            Some(classify_menu_line(line))
        })
        .collect()
}

fn classify_menu_line(raw_line: &str) -> ParsedMenuLine {
    let indent = raw_line
        .chars()
        .take_while(|ch| *ch == ' ' || *ch == '\t')
        .count();
    let content = raw_line.trim();
    let dash_count = content.chars().take_while(|ch| *ch == '-').count();
    let remainder = content[dash_count..].trim();

    let kind = match (dash_count, remainder.is_empty()) {
        (1, false) => ParsedMenuLineKind::Category1,
        (2, true) => ParsedMenuLineKind::Separator,
        (2, false) => ParsedMenuLineKind::Category2,
        _ => ParsedMenuLineKind::Item,
    };

    ParsedMenuLine {
        kind,
        content: remainder.to_string(),
        indent,
    }
}

fn parse_submenu_header(content: &str) -> (String, Vec<String>, Option<String>, bool) {
    let (label_with_id, extensions) = if let Some((label, filter_string)) = content.split_once('|') {
        let extensions = filter_string
            .split_whitespace()
            .filter_map(|token| {
                let token = token.trim().trim_start_matches('.').to_ascii_lowercase();
                (!token.is_empty()).then_some(token)
            })
            .collect();
        (label, extensions)
    } else {
        (content, Vec::new())
    };

    let (title_raw, explicit_id, context_default) = if let Some((title, id)) = label_with_id.rsplit_once("::") {
        let title = title.trim();
        let id = id.trim();
        let (id, context_default) = match id.strip_prefix('@') {
            Some(stripped) => (stripped.trim(), true),
            None => (id, false),
        };
        if !title.is_empty() && !id.is_empty() {
            (title, Some(id.to_string()), context_default)
        } else {
            (label_with_id.trim(), None, false)
        }
    } else {
        (label_with_id.trim(), None, false)
    };

    let title = strip_mnemonic_suffix(title_raw).trim().to_string();
    (title, extensions, explicit_id, context_default)
}

fn split_item_line(content: &str) -> (String, String) {
    content
        .split_once('|')
        .map(|(label, command)| {
            (
                strip_mnemonic_suffix(label).trim().to_string(),
                command.trim().to_string(),
            )
        })
        .unwrap_or_else(|| {
            let trimmed = content.trim();
            (strip_mnemonic_suffix(trimmed).trim().to_string(), trimmed.to_string())
        })
}

fn parse_default_marker(label: &str) -> (&str, bool) {
    match label.strip_prefix('*') {
        Some(stripped) => (stripped.trim(), true),
        None => (label, false),
    }
}

fn parse_command(command: &str) -> Result<MenuActionCommandConfig, String> {
    let command = command.trim();
    if let Some(action) = command.strip_prefix('@') {
        return parse_builtin_action(action);
    }

    let kind = infer_command_kind(command);
    Ok(MenuActionCommandConfig {
        kind: kind.to_string(),
        provider: Some(command.to_string()),
        desktop_id: None,
    })
}

fn infer_command_kind(command: &str) -> &'static str {
    if looks_like_url(command) {
        "quick-url"
    } else if looks_like_path(command) {
        "quick-path"
    } else {
        "shell"
    }
}

fn looks_like_url(command: &str) -> bool {
    command.contains("://")
}

fn looks_like_path(command: &str) -> bool {
    command.starts_with("~/")
        || command.starts_with("/")
        || command.starts_with("./")
        || command.starts_with("../")
}

fn parse_builtin_action(action: &str) -> Result<MenuActionCommandConfig, String> {
    if let Some(provider) = action.strip_prefix("search:") {
        return Ok(MenuActionCommandConfig {
            kind: "search".into(),
            provider: Some(provider.trim().to_string()),
            desktop_id: None,
        });
    }

    match action.trim() {
        "open-url" | "open-file" | "open-directory" | "reveal-file" | "copy-text" | "copy-path" => {
            Ok(MenuActionCommandConfig {
                kind: action.trim().to_string(),
                provider: None,
                desktop_id: None,
            })
        }
        other => Err(format!("unsupported builtin action: @{other}")),
    }
}

fn strip_mnemonic_suffix(label: &str) -> Cow<'_, str> {
    if let Some(start) = label.rfind("(&")
        && label.ends_with(')')
        && label[start + 2..label.len() - 1].chars().count() == 1
    {
        return Cow::Owned(label[..start].trim_end().to_string());
    }

    Cow::Borrowed(label)
}

pub fn render_quickmenu_command_template(template: &str) -> String {
    let now = Local::now();
    template
        .replace("%A_YYYY%", &now.format("%Y").to_string())
        .replace("%A_MM%", &now.format("%m").to_string())
        .replace("%A_DD%", &now.format("%d").to_string())
        .replace("%A_Hour%", &now.format("%H").to_string())
        .replace("%A_Min%", &now.format("%M").to_string())
        .replace("%A_Sec%", &now.format("%S").to_string())
}

fn submenu_action(label: &str, submenu_id: &str, counter: &mut usize) -> MenuActionConfig {
    MenuActionConfig {
        id: sanitize_id(label, "menu", counter),
        label: label.to_string(),
        icon: None,
        default: None,
        submenu: Some(submenu_id.to_string()),
        command: None,
    }
}

fn separator_action(counter: &mut usize) -> MenuActionConfig {
    MenuActionConfig {
        id: next_action_id("separator", counter),
        label: "---".into(),
        icon: None,
        default: None,
        submenu: None,
        command: Some(MenuActionCommandConfig {
            kind: "separator".into(),
            provider: None,
            desktop_id: None,
        }),
    }
}

fn actions_for_indented_line<'a>(
    line: &ParsedMenuLine,
    menus: &'a mut [MenuDefinition],
    level1_index: Option<usize>,
    level1_child_indent: &mut Option<usize>,
    level2_index: &mut Option<usize>,
    level2_child_indent: &mut Option<usize>,
    no_parent_error: &str,
) -> Result<&'a mut Vec<MenuActionConfig>, String> {
    let Some(level1) = level1_index else {
        return Err(no_parent_error.to_string());
    };

    match *level1_child_indent {
        Some(indent) if line.indent < indent => Err(format!("submenu child indentation is too shallow: {}", line.content)),
        Some(indent) if line.indent == indent => {
            *level2_index = None;
            *level2_child_indent = None;
            Ok(&mut menus[level1].actions)
        }
        Some(indent) => {
            let Some(level2) = *level2_index else {
                return Err(format!("deeply indented item requires a second-level submenu: {}", line.content));
            };
            match *level2_child_indent {
                Some(level2_indent) if line.indent != level2_indent => Err(format!(
                    "third-level indentation is not supported: {}",
                    line.content
                )),
                Some(_) => Ok(&mut menus[level2].actions),
                None if line.indent > indent => {
                    *level2_child_indent = Some(line.indent);
                    Ok(&mut menus[level2].actions)
                }
                None => Err(format!("deeply indented item requires a second-level submenu: {}", line.content)),
            }
        }
        None => {
            *level1_child_indent = Some(line.indent);
            *level2_index = None;
            *level2_child_indent = None;
            Ok(&mut menus[level1].actions)
        }
    }
}


fn sanitize_id(label: &str, prefix: &str, counter: &mut usize) -> String {
    let mut id = label
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    if id.is_empty() {
        id = format!("{prefix}-{}", *counter);
    }

    *counter += 1;
    id
}

fn next_submenu_id(counter: &mut usize) -> String {
    let id = format!("submenu-{}", *counter);
    *counter += 1;
    id
}

fn next_action_id(prefix: &str, counter: &mut usize) -> String {
    let id = format!("{prefix}-{}", *counter);
    *counter += 1;
    id
}

#[cfg(test)]
mod tests {
    use super::{
        bundled_config_dir_for, config_paths_from, load, parse_ini_menu, render_quickmenu_command_template,
        resolve_config_sources,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    #[test]
    fn prefers_sibling_config_directory_when_present() {
        let temp = TempDir::new().unwrap();
        let bin_dir = temp.path().join("release");
        let config_dir = bin_dir.join("config");
        fs::create_dir_all(&config_dir).unwrap();
        let current_exe = bin_dir.join("kanyrun");

        assert_eq!(
            bundled_config_dir_for(Some(&current_exe), Path::new("/repo")),
            config_dir
        );
    }

    #[test]
    fn falls_back_to_repo_assets_when_sibling_config_directory_is_absent() {
        assert_eq!(
            bundled_config_dir_for(None, Path::new("/repo")),
            std::path::PathBuf::from("/repo/assets/config")
        );
    }

    #[test]
    fn builds_xdg_user_and_bundled_paths() {
        let paths = config_paths_from(
            Some("/tmp/xdg".into()),
            Some("/tmp/home".into()),
            "/tmp/repo".into(),
        );

        assert_eq!(paths.user_dir, std::path::PathBuf::from("/tmp/xdg/kanyrun"));
        assert_eq!(paths.bundled_dir, std::path::PathBuf::from("/tmp/repo/assets/config"));
    }

    #[test]
    fn prefers_menu_override_when_provided() {
        let paths = config_paths_from(Some("/tmp/xdg".into()), None, "/tmp/repo".into());
        let resolved = resolve_config_sources(&paths, None, Some(Path::new("/tmp/custom/menu.ini")));

        assert_eq!(resolved.menu_file, PathBuf::from("/tmp/custom/menu.ini"));
    }

    #[test]
    fn honors_default_menu_file_from_config() {
        let paths = config_paths_from(Some("/tmp/xdg".into()), Some("/tmp/home".into()), "/tmp/repo".into());
        let resolved = resolve_config_sources(
            &paths,
            Some(&super::MenusConfig {
                default_file: Some("menu-work.ini".into()),
                default_id: None,
                fallback_id: None,
            }),
            None,
        );

        assert_eq!(resolved.menu_file, PathBuf::from("/tmp/xdg/kanyrun/menu-work.ini"));
    }

    #[test]
    fn falls_back_to_bundled_files_when_user_files_are_missing() {
        let temp = TempDir::new().unwrap();
        let repo_root = temp.path().join("repo");
        let bundled = repo_root.join("assets/config");
        fs::create_dir_all(&bundled).unwrap();
        fs::write(bundled.join("config.toml"), "[providers.duckduckgo]\nlabel='DuckDuckGo'\nurl='https://duckduckgo.com/?q={query}'\ndefault=true\n").unwrap();
        fs::write(bundled.join("menu.ini"), "Search|@search:duckduckgo\n").unwrap();

        let paths = config_paths_from(Some(temp.path().join("xdg")), None, repo_root);
        let resolved = resolve_config_sources(&paths, None, None);

        assert!(resolved.config_toml.ends_with("assets/config/config.toml"));
        assert!(resolved.menu_file.ends_with("assets/config/menu.ini"));
    }

    #[test]
    fn loads_user_config_and_falls_back_per_file() {
        let temp = TempDir::new().unwrap();
        let repo_root = temp.path().join("repo");
        let bundled = repo_root.join("assets/config");
        let user = temp.path().join("xdg/kanyrun");
        fs::create_dir_all(&bundled).unwrap();
        fs::create_dir_all(&user).unwrap();

        fs::write(bundled.join("config.toml"), "[providers.duckduckgo]\nlabel='DuckDuckGo'\nurl='https://duckduckgo.com/?q={query}'\ndefault=true\n").unwrap();
        fs::write(bundled.join("menu.ini"), "Search|@search:duckduckgo\n").unwrap();
        fs::write(user.join("config.toml"), "[providers.github]\nlabel='GitHub'\nurl='https://github.com/search?q={query}'\n").unwrap();

        let paths = config_paths_from(Some(temp.path().join("xdg")), None, repo_root);
        let loaded = load(&paths, None).unwrap();

        assert!(loaded.app.providers.contains_key("github"));
        assert_eq!(loaded.menu.menus[0].actions[0].label, "Search");
        assert!(loaded.sources.config_toml.ends_with("xdg/kanyrun/config.toml"));
        assert!(loaded.sources.menu_file.ends_with("assets/config/menu.ini"));
    }

    #[test]
    fn parses_indented_quickmenu_children() {
        let menu = parse_ini_menu(
            "*Search|@search:duckduckgo\n-Open With\n    Kate|kate %f\n    --Editors|txt md rs\n        VSCode|code %f\n    --\n    Copy path|@copy-path\n",
        )
        .unwrap();

        assert_eq!(menu.menus[0].actions.len(), 2);
        assert_eq!(menu.menus[1].actions.len(), 4);
        assert_eq!(menu.menus[2].actions.len(), 1);
        assert_eq!(menu.menus[1].actions[1].submenu.as_deref(), Some("submenu-1"));
        assert_eq!(menu.menus[2].actions[0].label, "VSCode");
    }

    #[test]
    fn rejects_unindented_children_inside_submenu() {
        let error = parse_ini_menu("-Open With\nKate|kate %f\n").unwrap_err();

        assert!(error.contains("submenu child must be indented"));
    }


    #[test]
    fn strips_quickmenu_mnemonic_suffixes() {
        let menu = parse_ini_menu("谷歌(&G)|https://www.google.com/search?q=%s\n-常用(&A)\n").unwrap();

        assert_eq!(menu.menus[0].actions[0].label, "谷歌");
        assert_eq!(menu.menus[1].title, "常用");
    }

    #[test]
    fn infers_quickmenu_url_and_path_commands() {
        let menu = parse_ini_menu("Google|https://example.com?q=%s\nApps|~/Apps\n").unwrap();

        assert_eq!(menu.menus[0].actions[0].command.as_ref().unwrap().kind, "quick-url");
        assert_eq!(menu.menus[0].actions[1].command.as_ref().unwrap().kind, "quick-path");
    }

    #[test]
    fn parses_context_default_submenu_ids() {
        let menu = parse_ini_menu("-编辑::@editor|txt toml\n    Kwrite|kwrite %f\n").unwrap();

        let editor = menu.menus.iter().find(|menu| menu.id == "editor").unwrap();
        assert!(editor.context_default);
        assert_eq!(editor.extensions, vec!["txt", "toml"]);
        assert_eq!(menu.menus[0].actions[0].submenu.as_deref(), Some("editor"));
    }

    #[test]
    fn renders_quickmenu_time_variables() {
        let rendered = render_quickmenu_command_template("%A_YYYY%-%A_MM%-%A_DD% %A_Hour%:%A_Min%:%A_Sec%");

        assert_eq!(rendered.len(), 19);
        assert_eq!(&rendered[4..5], "-");
        assert_eq!(&rendered[7..8], "-");
        assert_eq!(&rendered[10..11], " ");
        assert_eq!(&rendered[13..14], ":");
        assert_eq!(&rendered[16..17], ":");
    }

    #[test]
    fn reads_legacy_menu_toml() {
        let temp = TempDir::new().unwrap();
        let repo_root = temp.path().join("repo");
        let bundled = repo_root.join("assets/config");
        fs::create_dir_all(&bundled).unwrap();
        fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/config/config.toml"),
            bundled.join("config.toml"),
        )
        .unwrap();
        fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/config/menu.toml"),
            bundled.join("menu.toml"),
        )
        .unwrap();

        let loaded = load(
            &config_paths_from(Some(temp.path().join("xdg")), None, repo_root),
            None,
        )
        .unwrap();

        assert!(loaded.menu.menus.iter().any(|menu| menu.id == "selectedtext"));
        assert!(loaded
            .menu
            .menus
            .iter()
            .flat_map(|menu| menu.actions.iter())
            .any(|action| action.command.as_ref().map(|command| command.kind.as_str()) == Some("search")));
    }
}
