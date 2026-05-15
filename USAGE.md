# Kanyrun Usage Guide

## Bundle Contents

- `kanyrun`: main program
- `kanyrun-ui`: menu UI program
- `config/config.toml`: default rule configuration
- `config/menu.ini`: default quickmenu-style menu
- `config/config.sample.toml`: editable configuration sample
- `config/menu_sample.ini`: editable menu sample
- `install.sh`: installer
- `uninstall.sh`: uninstaller
- `kanyrun-open.sh`: generic launcher script for opening a menu after selecting external files or directories

## Installation

Run this inside the `release/` directory:

```sh
bash install.sh
```

The installer writes to:

- Programs: `~/.local/share/kanyrun/`
- Command symlink: `~/.local/bin/kanyrun`
- Bundled default config: `~/.local/share/kanyrun/config/`
- User config: `~/.config/kanyrun/`
- Sample files: `~/.local/share/kanyrun/samples/`
- Generic external launcher: `~/.local/share/kanyrun/kanyrun-open.sh`

By default, the installer:

- Always updates programs, bundled default config, sample files, and documentation.
- Initializes `~/.config/kanyrun/config.toml` only when the user config does not exist.
- Initializes `~/.config/kanyrun/menu.ini` only when the user has no `menu.ini`, `menu.conf`, or `menu.toml`.

This means repeated `install.sh` runs do not overwrite your active menu configuration.

## Uninstall

The default uninstall removes programs and command links, but keeps user config:

```sh
bash uninstall.sh
```

To also remove user config under `~/.config/kanyrun/`:

```sh
bash uninstall.sh --purge-config
```

## Run

After installation:

```sh
kanyrun
```

If `~/.local/bin` is not in your `PATH`, run the binary directly:

```sh
~/.local/share/kanyrun/kanyrun
```

## Common Commands

- `kanyrun`: show the default root menu.
- `kanyrun --root-menu`: show the default root menu without reading selection or clipboard.
- `kanyrun --from-primary --menu`: read primary selection but still use the default root menu.
- `kanyrun --text 'hello world'`: pass text explicitly.
- `kanyrun --text 'https://example.com'`: pass a URL explicitly.
- `kanyrun --files /path/a /path/b`: pass one or more files or directories explicitly.
- `kanyrun --menu --files /path/a`: force a menu for file context.
- `kanyrun --test`: show the default menu for UI testing.
- `kanyrun --menu-file ~/.config/kanyrun/menu-work.ini --test`: test another menu file.

Show help:

```sh
kanyrun --help
```

## Configuration Files

Kanyrun uses two config files by default:

- `~/.config/kanyrun/config.toml`
- `~/.config/kanyrun/menu.ini`

Their roles:

- `config.toml` controls rules, providers, UI switches, and the default menu file.
- `menu.ini` controls the quickmenu / RunAny-style menu structure.

Key fields:

```toml
[ui]
show_icons = true

[menus]
default_file = "menu.ini"
default_id = "root"
fallback_id = "root"
```

- `show_icons = false` hides menu icons.
- `default_file` switches to another `menu.ini`.
- `default_id` defaults to `root`.
- `fallback_id` is used when no rule matches, or when the matched menu becomes empty after extension filtering.

If `default_file` points to a missing file, runtime falls back to the bundled default `menu.ini`.

## menu.ini Syntax

The format follows quickmenu / RunAny style. Child items must be indented:

```ini
*Google|@search:google
Copy text|@copy-text

-Common::common
    Documents|~/Documents

-Text file::@text-file|txt md rs
    *Kate|kate %f
    VS Code|code %f

-File::@file
    *Open|@open-file
    Show in folder|@reveal-file

-Directory::@directory
    *Open folder|@open-file

-Files::@file-list
    *Open parent folder|@open-directory
```

Rules:

- `Item|Command`: normal menu item.
- `-Submenu::id`: first-level submenu.
- `--Submenu::id`: second-level submenu.
- `--`: separator.
- `*Item|Command`: default highlighted item.
- `; ...`: comment.

Additional notes:

- Submenu items must be indented with either 4 spaces or a tab.
- `::id` declares a stable menu ID, so `config.toml` rules can reference menus such as `menu = "file"`.
- Non-`@` items appear in the default root menu.
- `::@id` declares a context-default menu. The internal ID is still `id`, but the entry is hidden from the default root menu.
- With `--menu`, Kanyrun prefers context-default menus such as `file-list`, `directory`, or an extension-matched `::@...|ext` menu. Text context uses the default root menu.
- If a single file does not match an extension menu, Kanyrun tries `::@file` / `::file`, then falls back to the default root menu.
- `-Submenu::@id|txt md rs` adds extension filtering to a context-default menu. The menu is preferred only when the current file context matches those extensions.
- If the target menu becomes empty after extension filtering, runtime falls back to `[menus].fallback_id`.

Built-in actions:

- `@search:<provider>`
- `@copy-text`
- `@copy-path`
- `@open-file`
- `@open-directory`
- `@reveal-file`

Shortcut command inference:

- `https://...` is treated as a URL action.
- `~/...`, `/...`, `./...`, and `../...` are treated as path actions.
- Everything else is handled as a shell command.

For menu-item-specific environment variables, write a shell command directly:

```ini
WPS|env QT_IM_MODULE=fcitx GTK_IM_MODULE=fcitx XMODIFIERS=@im=fcitx wps %f
qimgv|env QT_SCALE_FACTOR=1 qimgv %f
```

This affects only the current menu item and does not pollute other commands. `%f` is passed as a single shell argument safely, so file names with spaces work.

Supported placeholders:

- `%s`: current text
- `%f`: current file path
- `%A_YYYY%` `%A_MM%` `%A_DD%` `%A_Hour%` `%A_Min%` `%A_Sec%`
- `{text_preview}`: current text preview shown in text menus

References:

- `release/config/menu_sample.ini`
- `~/.local/share/kanyrun/samples/menu_sample.ini`

## Generic External Launcher

`kanyrun-open.sh` is used for any workflow that needs to select files or directories externally and then open a Kanyrun menu.

It supports two modes:

- **With arguments**: use the provided file and directory paths directly. `file:///...` URIs are also accepted.
- **Without arguments**: quickly try to read context from the current active window.
  1. If the active window is Dolphin, trigger copy for the current selection and read a fresh `text/uri-list`.
  2. If the active window is FSearch, send one copy shortcut and parse the fresh file path.
  3. If no fresh file or directory selection is found, fall back to `kanyrun --menu --from-primary`. Text in primary selection still shows the default root menu.

After paths are collected, the script:

- Filters invalid entries.
- Handles quoted paths, `file://localhost/...`, and `file:///...`.
- Normalizes paths to real absolute paths.
- Deduplicates paths.
- Calls `kanyrun --menu --files <path1> <path2> ...`.

For Dolphin's no-argument path, the script briefly changes the clipboard and tries to restore it after reading the selection.

## Resident Mode

`kanyrun` now connects to a resident daemon by default to reduce cold-start overhead during repeated launches.

Common parameters:

```sh
kanyrun --daemon-idle-timeout 300 --ui-idle-timeout 15
```

- `--daemon-idle-timeout <seconds>`: exit the daemon after this many seconds without requests.
- `--ui-idle-timeout <seconds>`: reap the warm UI child after this many seconds without requests.
- `--oneshot`: bypass the daemon and run once in the current process.

### KDE Shortcut Suggestions

- For the default menu: bind `~/.local/bin/kanyrun`.
- For context menus on selected or copied files/directories: bind `~/.local/share/kanyrun/kanyrun-open.sh`.

When triggered from a global shortcut inside Dolphin, the script first tries to read the current active Dolphin selection.

### Direct Arguments

If an external tool can pass file arguments itself, configure it directly:

```sh
~/.local/share/kanyrun/kanyrun-open.sh /path/a /path/b
```

For example, a Dolphin external tool can use:

```sh
~/.local/share/kanyrun/kanyrun-open.sh %F
```

`%F` means the selected file list.

The script also exports:

- `KANYRUN_SELECTED_COUNT`
- `KANYRUN_SELECTED_PATHS`
- `KANYRUN_SELECTED_FIRST`
- `KANYRUN_SELECTED_DIR`
- `KANYRUN_SELECTED_NAME`
- `KANYRUN_SELECTED_STEMS`

## Suggested Cleanup Before Uploading To GitHub

Recommended source repository contents:

- `src/`, `assets/`, `ui/`, `scripts/`, and release scripts, config samples, and docs under `release/`
- `Cargo.toml`, `Cargo.lock`, installer/uninstaller scripts, and project documentation

Recommended exclusions:

- Rust build output: `target/`
- Qt/CMake local build output: `.build/`, `ui/**/build/`, `ui/**/build-*/`
- Local caches and agent state: `.cache/`, `.agents/`, `.claude/`, `.codex/`
- External reference projects or personal work directories: `refers/`, `Finance/`
- Local binaries in release: `release/kanyrun`, `release/kanyrun-ui`

This repository already includes `.gitignore` rules for those paths. For binary distribution, prefer uploading packaged artifacts through GitHub Releases instead of committing them to the source repository.

## Rebuild The Release Bundle

After updating code in the source tree, regenerate and synchronize `release/`:

```sh
bash scripts/build-release.sh
```

The script automatically:

- Builds the Rust release binary.
- Builds the Qt UI release binary.
- Synchronizes the latest artifacts into `release/`.

## License

Kanyrun is licensed under the GNU General Public License v3.0. See `LICENSE` for the full license text.
