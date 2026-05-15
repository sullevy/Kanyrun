#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [ -x "$SCRIPT_DIR/kanyrun" ] && [ -x "$SCRIPT_DIR/kanyrun-ui" ]; then
    SOURCE_DIR="$SCRIPT_DIR"
else
    SOURCE_DIR="$SCRIPT_DIR/release"
fi
INSTALL_ROOT="${XDG_DATA_HOME:-$HOME/.local/share}/kanyrun"
BUNDLE_CONFIG_DIR="$INSTALL_ROOT/config"
BIN_DIR="${HOME}/.local/bin"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/kanyrun"
SAMPLES_DIR="$INSTALL_ROOT/samples"

if [ ! -x "$SOURCE_DIR/kanyrun" ] || [ ! -x "$SOURCE_DIR/kanyrun-ui" ]; then
    printf 'release bundle is missing binaries; run scripts/build-release.sh first\n' >&2
    exit 1
fi

mkdir -p "$INSTALL_ROOT" "$BUNDLE_CONFIG_DIR" "$BIN_DIR" "$CONFIG_DIR" "$SAMPLES_DIR"
install -m 0755 "$SOURCE_DIR/kanyrun" "$INSTALL_ROOT/kanyrun"
install -m 0755 "$SOURCE_DIR/kanyrun-ui" "$INSTALL_ROOT/kanyrun-ui"
install -m 0755 "$SOURCE_DIR/kanyrun-open.sh" "$INSTALL_ROOT/kanyrun-open.sh"
rm -f "$INSTALL_ROOT/get-path" "$BIN_DIR/get-path"
if [ -x "$SOURCE_DIR/get-file-path.sh" ]; then
    install -m 0755 "$SOURCE_DIR/get-file-path.sh" "$INSTALL_ROOT/get-file-path.sh"
fi
install -m 0644 "$SOURCE_DIR/config/config.toml" "$BUNDLE_CONFIG_DIR/config.toml"
install -m 0644 "$SOURCE_DIR/config/menu.ini" "$BUNDLE_CONFIG_DIR/menu.ini"
install -m 0644 "$SOURCE_DIR/config/menu_sample.ini" "$SAMPLES_DIR/menu_sample.ini"
install -m 0644 "$SOURCE_DIR/config/config.sample.toml" "$SAMPLES_DIR/config.sample.toml"
install -m 0644 "$SOURCE_DIR/使用说明.md" "$INSTALL_ROOT/使用说明.md"

ln -sfn "$INSTALL_ROOT/kanyrun" "$BIN_DIR/kanyrun"

if [ ! -f "$CONFIG_DIR/config.toml" ]; then
    install -m 0644 "$SOURCE_DIR/config/config.toml" "$CONFIG_DIR/config.toml"
fi

if [ ! -f "$CONFIG_DIR/menu.ini" ] && [ ! -f "$CONFIG_DIR/menu.conf" ] && [ ! -f "$CONFIG_DIR/menu.toml" ]; then
    install -m 0644 "$SOURCE_DIR/config/menu.ini" "$CONFIG_DIR/menu.ini"
fi

printf 'Installed kanyrun to %s\n' "$INSTALL_ROOT"
printf 'Bundled defaults: %s\n' "$BUNDLE_CONFIG_DIR"
printf 'Sample files: %s\n' "$SAMPLES_DIR"
printf 'Command symlink: %s/kanyrun\n' "$BIN_DIR"
printf 'Generic launcher: %s/kanyrun-open.sh\n' "$INSTALL_ROOT"
printf 'User config dir: %s\n' "$CONFIG_DIR"
printf '\n'
printf 'Next steps:\n'
printf '  1. Bind a KDE shortcut to: %s/kanyrun\n' "$BIN_DIR"
printf '  2. Edit %s/menu.ini for your real menu\n' "$CONFIG_DIR"
printf '  3. See %s/使用说明.md for menu.ini examples\n' "$INSTALL_ROOT"

case ":${PATH}:" in
    *":${BIN_DIR}:"*) ;;
    *) printf 'Note: %s is not in PATH yet; reopen your shell or add it manually.\n' "$BIN_DIR" ;;
esac
