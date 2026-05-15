#!/usr/bin/env bash
set -euo pipefail

INSTALL_ROOT="${XDG_DATA_HOME:-$HOME/.local/share}/kanyrun"
BIN_DIR="${HOME}/.local/bin"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/kanyrun"
PURGE_CONFIG="${1:-}"

rm -f "$INSTALL_ROOT/kanyrun" "$INSTALL_ROOT/kanyrun-ui" "$INSTALL_ROOT/kanyrun-open.sh" "$INSTALL_ROOT/get-path" "$INSTALL_ROOT/get-file-path.sh"
rm -rf "$INSTALL_ROOT/config"
rmdir "$INSTALL_ROOT" 2>/dev/null || true

if [ -L "$BIN_DIR/kanyrun" ]; then
    TARGET="$(readlink "$BIN_DIR/kanyrun")"
    if [ "$TARGET" = "$INSTALL_ROOT/kanyrun" ]; then
        rm -f "$BIN_DIR/kanyrun" "$BIN_DIR/get-path"
    fi
fi

if [ "$PURGE_CONFIG" = "--purge-config" ]; then
    rm -f "$CONFIG_DIR/config.toml" "$CONFIG_DIR/menu.ini" "$CONFIG_DIR/menu.conf" "$CONFIG_DIR/menu.toml"
    rmdir "$CONFIG_DIR" 2>/dev/null || true
fi

printf 'Removed kanyrun from %s\n' "$INSTALL_ROOT"
printf 'Removed command symlink if it pointed to %s/kanyrun\n' "$INSTALL_ROOT"
if [ "$PURGE_CONFIG" = "--purge-config" ]; then
    printf 'Removed user config dir: %s\n' "$CONFIG_DIR"
else
    printf 'Preserved user config dir: %s\n' "$CONFIG_DIR"
fi
