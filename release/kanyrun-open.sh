#!/usr/bin/env bash
set -euo pipefail

INSTALL_ROOT="${XDG_DATA_HOME:-$HOME/.local/share}/kanyrun"
KANYRUN_DAEMON_IDLE_TIMEOUT="${KANYRUN_DAEMON_IDLE_TIMEOUT:-300}"
KANYRUN_UI_IDLE_TIMEOUT="${KANYRUN_UI_IDLE_TIMEOUT:-15}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ -x "$INSTALL_ROOT/kanyrun" ]; then
    KANYRUN="$INSTALL_ROOT/kanyrun"
elif [ -x "$SCRIPT_DIR/kanyrun" ]; then
    KANYRUN="$SCRIPT_DIR/kanyrun"
elif command -v kanyrun >/dev/null 2>&1; then
    KANYRUN="$(command -v kanyrun)"
else
    printf 'kanyrun not found in installed location, next to this script, or in PATH\n' >&2
    exit 1
fi

if [ -x "$INSTALL_ROOT/get-path" ]; then
    GET_PATH="$INSTALL_ROOT/get-path"
elif [ -x "$SCRIPT_DIR/get-path" ]; then
    GET_PATH="$SCRIPT_DIR/get-path"
elif command -v get-path >/dev/null 2>&1; then
    GET_PATH="$(command -v get-path)"
else
    printf 'get-path not found in installed location, next to this script, or in PATH\n' >&2
    exit 1
fi

show_error() {
    local message="$1"

    if command -v notify-send >/dev/null 2>&1; then
        notify-send "Kanyrun" "$message"
    fi
    printf '%s\n' "$message" >&2
}

if ! mapfile -t paths < <("$GET_PATH" "$@" 2> >(while IFS= read -r line; do printf '%s\n' "$line" >&2; done)); then
    show_error 'No valid file or directory paths were found.'
    exit 1
fi

if [ "${#paths[@]}" -eq 0 ]; then
    show_error 'No valid file or directory paths were found.'
    exit 1
fi

export KANYRUN_SELECTED_COUNT="${#paths[@]}"
export KANYRUN_SELECTED_PATHS="$(printf '%s\n' "${paths[@]}")"
export KANYRUN_SELECTED_FIRST="${paths[0]}"
export KANYRUN_SELECTED_DIR="$(dirname "${paths[0]}")"
export KANYRUN_SELECTED_NAME="$(basename "${paths[0]}")"
export KANYRUN_SELECTED_STEMS="$(printf '%s\n' "${paths[@]}" | xargs -r -I{} basename "{}")"

exec "$KANYRUN" \
    --daemon-idle-timeout "$KANYRUN_DAEMON_IDLE_TIMEOUT" \
    --ui-idle-timeout "$KANYRUN_UI_IDLE_TIMEOUT" \
    --menu \
    --files "${paths[@]}"
