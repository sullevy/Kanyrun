#!/usr/bin/env bash
set -euo pipefail

KANYRUN_OPEN_TIMING="${KANYRUN_OPEN_TIMING:-1}"
KANYRUN_OPEN_TIMING_LOG="${KANYRUN_OPEN_TIMING_LOG:-/tmp/kanyrun-open-timing.log}"
KANYRUN_OPEN_CONTEXT_LOG="${KANYRUN_OPEN_CONTEXT_LOG:-0}"
TIMING_STARTED_RAW="${EPOCHREALTIME:-0.000000}"
TIMING_STARTED_US="${TIMING_STARTED_RAW/./}"
TIMING_LAST_US="$TIMING_STARTED_US"
TIMING_REQUEST_ID="$$.$TIMING_STARTED_US"
TIMING_SCRIPT_ARGC="$#"

INSTALL_ROOT="${XDG_DATA_HOME:-$HOME/.local/share}/kanyrun"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KANYRUN_DAEMON_IDLE_TIMEOUT="${KANYRUN_DAEMON_IDLE_TIMEOUT:-300}"
KANYRUN_UI_IDLE_TIMEOUT="${KANYRUN_UI_IDLE_TIMEOUT:-15}"
KANYRUN_DISMISS_EXISTING="${KANYRUN_DISMISS_EXISTING:-1}"
KANYRUN_MENU_PID_FILE="${KANYRUN_MENU_PID_FILE:-${XDG_RUNTIME_DIR:-/tmp}/kanyrun/ui.pid}"
KANYRUN_COPY_WAIT_ATTEMPTS="${KANYRUN_COPY_WAIT_ATTEMPTS:-${KANYRUN_GET_PATH_WAIT_ATTEMPTS:-6}}"
KANYRUN_COPY_WAIT_DELAY_MS="${KANYRUN_COPY_WAIT_DELAY_MS:-${KANYRUN_GET_PATH_WAIT_DELAY_MS:-8}}"
export KANYRUN_TIMING="${KANYRUN_TIMING:-$KANYRUN_OPEN_TIMING}"
export KANYRUN_TIMING_REQUEST_ID="$TIMING_REQUEST_ID"

timing_now_us() {
    local raw="${EPOCHREALTIME:-0.000000}"
    printf '%s' "${raw/./}"
}

timing_mark() {
    if [ "$KANYRUN_OPEN_TIMING" = "0" ]; then
        return
    fi

    local stage="$1"
    local detail="${2:-}"
    local now_us delta_us total_us

    now_us="$(timing_now_us)"
    delta_us=$((now_us - TIMING_LAST_US))
    total_us=$((now_us - TIMING_STARTED_US))
    TIMING_LAST_US="$now_us"

    printf 'now_us=%s req=%s stage=%s delta_us=%s total_us=%s argc=%s %s\n' \
        "$now_us" \
        "$TIMING_REQUEST_ID" \
        "$stage" \
        "$delta_us" \
        "$total_us" \
        "$TIMING_SCRIPT_ARGC" \
        "$detail" >> "$KANYRUN_OPEN_TIMING_LOG" || true
}

timing_value() {
    local value="${1:-}"
    value="${value//$'\n'/ }"
    value="${value//$'\r'/ }"
    value="${value// /_}"
    printf '%s' "$value"
}

find_program() {
    local name="$1"

    if [ -x "$INSTALL_ROOT/$name" ]; then
        printf '%s\n' "$INSTALL_ROOT/$name"
    elif [ -x "$SCRIPT_DIR/$name" ]; then
        printf '%s\n' "$SCRIPT_DIR/$name"
    elif command -v "$name" >/dev/null 2>&1; then
        command -v "$name"
    else
        return 1
    fi
}

show_error() {
    local message="$1"

    if command -v notify-send >/dev/null 2>&1; then
        notify-send "Kanyrun" "$message"
    fi
    printf '%s\n' "$message" >&2
}

dismiss_existing_menu() {
    if [ "$KANYRUN_DISMISS_EXISTING" = "0" ]; then
        timing_mark dismiss_existing_menu "status=disabled"
        return
    fi
    if [ ! -r "$KANYRUN_MENU_PID_FILE" ]; then
        timing_mark dismiss_existing_menu "status=no-pidfile"
        return
    fi

    local pid comm
    IFS= read -r pid < "$KANYRUN_MENU_PID_FILE" || pid=""
    if [[ ! "$pid" =~ ^[0-9]+$ ]]; then
        timing_mark dismiss_existing_menu "status=bad-pid"
        return
    fi
    if [ ! -r "/proc/$pid/comm" ]; then
        timing_mark dismiss_existing_menu "status=stale pid=$pid"
        return
    fi

    IFS= read -r comm < "/proc/$pid/comm" || comm=""
    if [ "$comm" != "kanyrun-ui" ]; then
        timing_mark dismiss_existing_menu "status=pid-mismatch pid=$pid comm=$(timing_value "$comm")"
        return
    fi

    if kill -USR1 "$pid" 2>/dev/null; then
        timing_mark dismiss_existing_menu "status=signaled pid=$pid"
    else
        timing_mark dismiss_existing_menu "status=signal-failed pid=$pid"
    fi
}

timing_mark init

KANYRUN="$(find_program kanyrun)" || {
    printf 'kanyrun not found in installed location, next to this script, or in PATH\n' >&2
    exit 1
}
timing_mark resolve_programs
dismiss_existing_menu

run_menu() {
    timing_mark exec_menu "paths=$#"
    exec "$KANYRUN" \
        --daemon-idle-timeout "$KANYRUN_DAEMON_IDLE_TIMEOUT" \
        --ui-idle-timeout "$KANYRUN_UI_IDLE_TIMEOUT" \
        --menu \
        "$@"
}

run_context_menu() {
    timing_mark exec_context_menu

    unset KANYRUN_SELECTED_COUNT
    unset KANYRUN_SELECTED_PATHS
    unset KANYRUN_SELECTED_FIRST
    unset KANYRUN_SELECTED_DIR
    unset KANYRUN_SELECTED_NAME
    unset KANYRUN_SELECTED_STEMS

    exec "$KANYRUN" \
        --daemon-idle-timeout "$KANYRUN_DAEMON_IDLE_TIMEOUT" \
        --ui-idle-timeout "$KANYRUN_UI_IDLE_TIMEOUT" \
        --menu \
        --from-primary
}

collect_paths() {
    if [ "$#" -gt 0 ]; then
        printf '%s\n' "$@"
    else
        collect_paths_from_copy_shortcut
    fi
}

clear_clipboard() {
    command -v wl-copy >/dev/null 2>&1 || return 0
    wl-copy --clear >/dev/null 2>&1 || true
}

send_copy_shortcut() {
    if command -v dotoolc >/dev/null 2>&1 && dotoolc >/dev/null 2>&1 <<< "key ctrl+c"; then
        printf '%s\n' dotoolc
        return 0
    fi
    if command -v dotool >/dev/null 2>&1 && dotool >/dev/null 2>&1 <<< "key ctrl+c"; then
        printf '%s\n' dotool
        return 0
    fi
    if command -v wtype >/dev/null 2>&1 && wtype -M ctrl -k c -m ctrl >/dev/null 2>&1; then
        printf '%s\n' wtype
        return 0
    fi
    if command -v kdotool >/dev/null 2>&1 && kdotool key ctrl+c >/dev/null 2>&1; then
        printf '%s\n' kdotool
        return 0
    fi
    return 1
}

sleep_ms() {
    local ms="$1"
    if [ "$ms" -le 0 ]; then
        return
    fi

    local seconds millis
    seconds=$((ms / 1000))
    millis=$((ms % 1000))
    sleep "${seconds}.$(printf '%03d' "$millis")"
}

append_uri_candidates() {
    local raw="$1"
    local line

    while IFS= read -r line; do
        line="${line%$'\r'}"
        if [ -z "$line" ] || [[ "$line" == \#* ]]; then
            continue
        fi
        if [[ "$line" == file://* ]]; then
            path_array+=("$line")
        fi
    done <<< "$raw"
}

append_plain_candidates() {
    local raw="$1"
    local line

    while IFS= read -r line; do
        line="${line%$'\r'}"
        if [ -z "$line" ] || [ "$line" = "copy" ] || [ "$line" = "cut" ]; then
            continue
        fi
        if [ "$line" = "x-special/nautilus-clipboard" ]; then
            continue
        fi
        if [[ "$line" == file://* ]] || [ -e "$line" ]; then
            path_array+=("$line")
        fi
    done <<< "$raw"
}

collect_paths_from_copy_shortcut() {
    local tool attempt raw

    if ! command -v wl-paste >/dev/null 2>&1; then
        timing_mark clipboard_poll "tool=wl-paste status=missing"
        return 1
    fi

    path_array=()
    clear_clipboard
    timing_mark clear_clipboard

    tool="$(send_copy_shortcut)" || return 1
    timing_mark send_copy "tool=$tool"

    for ((attempt = 1; attempt <= KANYRUN_COPY_WAIT_ATTEMPTS; attempt++)); do
        path_array=()
        raw="$(wl-paste --no-newline --type text/uri-list 2>/dev/null || true)"
        append_uri_candidates "$raw"
        if [ "${#path_array[@]}" -gt 0 ]; then
            timing_mark clipboard_poll "attempt=$attempt source=uri-list count=${#path_array[@]}"
            printf '%s\n' "${path_array[@]}"
            return 0
        fi

        raw="$(wl-paste --no-newline --type text/plain 2>/dev/null || true)"
        append_plain_candidates "$raw"
        if [ "${#path_array[@]}" -gt 0 ]; then
            timing_mark clipboard_poll "attempt=$attempt source=text-plain count=${#path_array[@]}"
            printf '%s\n' "${path_array[@]}"
            return 0
        fi

        sleep_ms "$KANYRUN_COPY_WAIT_DELAY_MS"
    done

    timing_mark clipboard_poll "attempts=$KANYRUN_COPY_WAIT_ATTEMPTS source=none count=0"
    return 1
}

log_trigger_context() {
    if [ "$KANYRUN_OPEN_TIMING" = "0" ] || [ "$KANYRUN_OPEN_CONTEXT_LOG" = "0" ]; then
        return
    fi
    if ! command -v kdotool >/dev/null 2>&1; then
        timing_mark trigger_context "tool=missing"
        return
    fi

    local window_id window_class window_name window_pid process_name detail
    window_id="$(kdotool getactivewindow 2>/dev/null || true)"
    window_id="$(timing_value "$window_id")"
    if [ -z "$window_id" ]; then
        timing_mark trigger_context "tool=kdotool window=empty"
        return
    fi

    window_class="$(kdotool getwindowclassname "$window_id" 2>/dev/null || true)"
    window_name="$(kdotool getwindowname "$window_id" 2>/dev/null || true)"
    window_pid="$(kdotool getwindowpid "$window_id" 2>/dev/null || true)"
    window_class="$(timing_value "$window_class")"
    window_name="$(timing_value "$window_name")"
    window_pid="$(timing_value "$window_pid")"
    process_name=""
    if [[ "$window_pid" =~ ^[0-9]+$ ]] && [ -r "/proc/$window_pid/comm" ]; then
        IFS= read -r process_name < "/proc/$window_pid/comm" || true
        process_name="$(timing_value "$process_name")"
    fi

    detail="tool=kdotool window=$window_id class=$window_class pid=$window_pid process=$process_name title=$window_name"
    timing_mark trigger_context "$detail"
}

# ---- main ----

output_lines=()
mapfile -t output_lines < <(collect_paths "$@" 2>/dev/null || true)
timing_mark collect_paths "lines=${#output_lines[@]}"

path_array=("${output_lines[@]}")
count="${#path_array[@]}"
timing_mark parse_paths "count=$count"

if [ "$count" -eq 0 ]; then
    if [ "$#" -eq 0 ]; then
        log_trigger_context
        run_context_menu
    fi
    timing_mark error_no_paths
    show_error 'No valid file or directory paths were found.'
    exit 1
fi

selected_paths=""
selected_stems=""
for path in "${path_array[@]}"; do
    if [ -n "$selected_paths" ]; then
        selected_paths+=$'\n'
        selected_stems+=$'\n'
    fi
    selected_paths+="$path"
    selected_stems+="${path##*/}"
done

selected_first="${path_array[0]}"
selected_dir="${selected_first%/*}"
if [ "$selected_dir" = "$selected_first" ]; then
    selected_dir="."
elif [ -z "$selected_dir" ]; then
    selected_dir="/"
fi

export KANYRUN_SELECTED_COUNT="$count"
export KANYRUN_SELECTED_PATHS="$selected_paths"
export KANYRUN_SELECTED_FIRST="$selected_first"
export KANYRUN_SELECTED_DIR="$selected_dir"
export KANYRUN_SELECTED_NAME="${selected_first##*/}"
export KANYRUN_SELECTED_STEMS="$selected_stems"
timing_mark export_selection_env "count=$count"

run_menu --files "${path_array[@]}"
