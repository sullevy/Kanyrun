#!/usr/bin/env bash
set -euo pipefail

KANYRUN_TIMING_FILE="${KANYRUN_TIMING_FILE:-}"
KANYRUN_TIMING_RUN_ID="${KANYRUN_TIMING_RUN_ID:-}"

now_us() {
    if [ -n "${EPOCHREALTIME-}" ]; then
        printf '%s\n' "${EPOCHREALTIME/./}"
    else
        date +%s%6N
    fi
}

elapsed_ms() {
    local start_us="$1"
    local end_us="${2:-$(now_us)}"

    printf '%s\n' "$(( (10#$end_us - 10#$start_us) / 1000 ))"
}

timing_enabled() {
    [ -n "$KANYRUN_TIMING_FILE" ]
}

timing_log() {
    local scope="$1"
    local event="$2"
    shift 2

    timing_enabled || return 0

    printf 'ts_us=%s run=%s scope=%s event=%s' \
        "$(now_us)" "$KANYRUN_TIMING_RUN_ID" "$scope" "$event" >> "$KANYRUN_TIMING_FILE"
    if [ "$#" -gt 0 ]; then
        printf ' %s' "$*" >> "$KANYRUN_TIMING_FILE"
    fi
    printf '\n' >> "$KANYRUN_TIMING_FILE"
}

show_output() {
    local message="$1"

    if command -v kdialog >/dev/null 2>&1; then
        if kdialog --title "Kanyrun file-path debug" --msgbox "$message"; then
            return
        fi
    fi

    if command -v notify-send >/dev/null 2>&1; then
        notify-send "Kanyrun file-path debug" "$message"
    fi

    printf '%s\n' "$message"
}

has_command() {
    command -v "$1" >/dev/null 2>&1
}

get_active_window_id() {
    has_command kdotool || return 1
    kdotool getactivewindow 2>/dev/null
}

get_active_window_class() {
    local window_id

    window_id="$(get_active_window_id)" || return 1
    [ -n "$window_id" ] || return 1
    kdotool getwindowclassname "$window_id" 2>/dev/null
}

get_active_window_pid() {
    local window_id

    window_id="$(get_active_window_id)" || return 1
    [ -n "$window_id" ] || return 1
    kdotool getwindowpid "$window_id" 2>/dev/null
}

window_class_is_dolphin() {
    local class_name

    class_name="${1,,}"
    case "$class_name" in
        *org.kde.dolphin*|*dolphin*)
            return 0
            ;;
    esac

    return 1
}

window_class_is_terminal() {
    local class_name

    class_name="${1,,}"
    case "$class_name" in
        *konsole*|*kitty*|*alacritty*|*wezterm*|*ghostty*|*foot*|*terminal*|*xterm*|*st*|*gnome-console*|*yakuake*)
            return 0
            ;;
    esac

    return 1
}

send_copy_shortcut() {
    local active_class="$1"
    local chord="ctrl+c"

    if window_class_is_terminal "$active_class"; then
        chord="ctrl+shift+c"
    fi

    if has_command dotoolc; then
        printf 'key %s\n' "$chord" | dotoolc >/dev/null 2>&1
        return $?
    fi

    if has_command dotool; then
        printf 'key %s\n' "$chord" | dotool >/dev/null 2>&1
        return $?
    fi

    if has_command wtype; then
        if [ "$chord" = "ctrl+shift+c" ]; then
            wtype -M ctrl -M shift -k c -m shift -m ctrl >/dev/null 2>&1
        else
            wtype -M ctrl -k c -m ctrl >/dev/null 2>&1
        fi
        return $?
    fi

    return 1
}

canonicalize_path() {
    local path="$1"

    if [ -e "$path" ]; then
        realpath -e -- "$path"
        return
    fi

    return 1
}

decode_file_uri() {
    python3 -c 'import sys, urllib.parse; print(urllib.parse.unquote(urllib.parse.urlsplit(sys.argv[1]).path))' "$1"
}

canonicalize_candidate() {
    local value="$1"

    value="${value%$'\r'}"
    value="${value#\"}"
    value="${value%\"}"
    value="${value#\'}"
    value="${value%\'}"
    value="${value#file://localhost}"

    case "$value" in
        file://*)
            decode_file_uri "$value"
            ;;
        *)
            printf '%s\n' "$value"
            ;;
    esac
}

collect_paths() {
    local seen="|"
    local raw candidate normalized

    for raw in "$@"; do
        [ -n "$raw" ] || continue
        candidate="$(canonicalize_candidate "$raw")"
        [ -n "$candidate" ] || continue
        if ! normalized="$(canonicalize_path "$candidate")"; then
            continue
        fi
        case "$seen" in
            *"|$normalized|"*)
                continue
                ;;
        esac
        seen+="$normalized|"
        printf '%s\n' "$normalized"
    done
}

read_uri_list_raw() {
    wl-paste --no-newline --type text/uri-list 2>/dev/null || true
}

read_plain_text_raw() {
    wl-paste --no-newline --type text/plain 2>/dev/null || true
}

read_primary_text_raw() {
    wl-paste --no-newline --primary --type text/plain 2>/dev/null || true
}

read_html_raw() {
    wl-paste --no-newline --type text/html 2>/dev/null || true
}

split_nonempty_lines() {
    local data="$1"
    local line

    [ -n "$data" ] || return 1

    while IFS= read -r line; do
        [ -n "$line" ] && printf '%s\n' "$line"
    done <<< "$data"
}

read_uri_candidates() {
    local data="$1"
    local line decoded

    [ -n "$data" ] || return 1

    while IFS= read -r line; do
        [ -n "$line" ] || continue
        case "$line" in
            \#*)
                continue
                ;;
            file://*)
                decoded="$(decode_file_uri "$line")"
                [ -n "$decoded" ] && printf '%s\n' "$decoded"
                ;;
            *)
                printf '%s\n' "$line"
                ;;
        esac
    done <<< "$data"
}

html_to_lines() {
    python3 -c '
import html
import re
import sys

text = sys.argv[1]
text = re.sub(r"<!--.*?-->", "", text, flags=re.S)
text = re.sub(r"<br\\s*/?>", "\\n", text, flags=re.I)
text = re.sub(r"</(p|div|li|tr|h[1-6])\\s*>", "\\n", text, flags=re.I)
text = re.sub(r"<[^>]+>", "", text)
text = html.unescape(text)
for line in text.splitlines():
    line = line.strip()
    if line:
        print(line)
' "$1"
}

looks_like_html() {
    local data="$1"

    case "$data" in
        *"<!DOCTYPE HTML"*|*"<html"*|*"<body"*|*"</"*)
            return 0
            ;;
    esac

    return 1
}

save_clipboard_state() {
    local state_dir="$1"
    local type

    if ! wl-paste --list-types > "$state_dir/types" 2>/dev/null; then
        return 1
    fi

    while IFS= read -r type; do
        [ -n "$type" ] || continue
        if wl-paste --type "$type" > "$state_dir/content" 2>/dev/null; then
            printf 'restore\n' > "$state_dir/mode"
            printf '%s\n' "$type" > "$state_dir/type"
            return 0
        fi
    done < "$state_dir/types"

    printf 'clear\n' > "$state_dir/mode"
}

restore_clipboard_state() {
    local state_dir="$1"
    local mode type

    [ -f "$state_dir/mode" ] || return 0
    mode="$(<"$state_dir/mode")"

    case "$mode" in
        restore)
            [ -f "$state_dir/type" ] || return 0
            [ -f "$state_dir/content" ] || return 0
            type="$(<"$state_dir/type")"
            [ -n "$type" ] || return 0
            wl-copy --type "$type" < "$state_dir/content" 2>/dev/null || true
            ;;
        clear)
            wl-copy --clear 2>/dev/null || true
            ;;
    esac
}

list_dolphin_windows() {
    local service="$1"

    busctl --user --no-pager --list tree "$service" 2>/dev/null \
        | awk '$0 ~ /^\/dolphin\/Dolphin_[0-9]+$/ { print $0 }'
}

busctl_bool_reply() {
    local service="$1"
    local object_path="$2"
    local interface="$3"
    local method="$4"
    shift 4

    busctl --user call "$service" "$object_path" "$interface" "$method" "$@" 2>/dev/null \
        | awk 'NR == 1 { print $NF }'
}

busctl_string_property() {
    local service="$1"
    local object_path="$2"
    local interface="$3"
    local property="$4"

    busctl --user get-property "$service" "$object_path" "$interface" "$property" 2>/dev/null \
        | python3 -c '
import ast
import sys

line = sys.stdin.read().strip()
if not line:
    sys.exit(0)
kind, _, value = line.partition(" ")
if kind != "s":
    sys.exit(0)
try:
    print(ast.literal_eval(value))
except Exception:
    print("")
'
}

dolphin_window_directory() {
    local service="$1"
    local object_path="$2"
    local title

    title="$(busctl_string_property "$service" "$object_path" org.qtproject.Qt.QWidget windowTitle)"
    title="${title% — Dolphin}"

    if [ -d "$title" ]; then
        printf '%s\n' "$title"
    fi
}

active_dolphin_service() {
    local active_pid="$1"
    local service

    [ -n "$active_pid" ] || return 1
    service="org.kde.dolphin-${active_pid}"

    busctl --user --no-pager --list tree "$service" >/dev/null 2>&1 || return 1
    printf '%s\n' "$service"
}

active_dolphin_object_path() {
    local service="$1"
    local object_path first_object=""

    while IFS= read -r object_path; do
        [ -n "$object_path" ] || continue
        [ -n "$first_object" ] || first_object="$object_path"

        if [ "$(busctl_bool_reply "$service" "$object_path" org.kde.dolphin.MainWindow isActiveWindow)" = "true" ]; then
            printf '%s\n' "$object_path"
            return 0
        fi
    done < <(list_dolphin_windows "$service")

    [ -n "$first_object" ] || return 1
    printf '%s\n' "$first_object"
}

resolve_dolphin_location_candidates() {
    local base_dir="$1"
    local plain_data="$2"
    local html_data="$3"
    local source_data line

    if [ -n "$html_data" ]; then
        source_data="$html_data"
    else
        source_data="$plain_data"
    fi

    [ -n "$source_data" ] || return 1

    if looks_like_html "$source_data"; then
        source_data="$(html_to_lines "$source_data" || true)"
    fi

    [ -n "$source_data" ] || return 1

    while IFS= read -r line; do
        [ -n "$line" ] || continue
        case "$line" in
            file://*|/*)
                printf '%s\n' "$line"
                ;;
            *)
                if [ -n "$base_dir" ]; then
                    printf '%s/%s\n' "$base_dir" "$line"
                fi
                ;;
        esac
    done <<< "$source_data"
}

resolve_paths_from_uri_raw() {
    local raw_uri="$1"
    local -a uri_candidates=()
    local -a resolved_paths=()

    [ -n "$raw_uri" ] || return 1

    mapfile -t uri_candidates < <(read_uri_candidates "$raw_uri")
    [ "${#uri_candidates[@]}" -gt 0 ] || return 1

    mapfile -t resolved_paths < <(collect_paths "${uri_candidates[@]}")
    [ "${#resolved_paths[@]}" -gt 0 ] || return 1

    printf '%s\n' "${resolved_paths[@]}"
}

resolve_paths_from_plain_raw() {
    local raw_plain="$1"
    local -a plain_candidates=()
    local -a resolved_paths=()

    [ -n "$raw_plain" ] || return 1

    mapfile -t plain_candidates < <(split_nonempty_lines "$raw_plain")
    [ "${#plain_candidates[@]}" -gt 0 ] || return 1

    mapfile -t resolved_paths < <(collect_paths "${plain_candidates[@]}")
    [ "${#resolved_paths[@]}" -gt 0 ] || return 1

    printf '%s\n' "${resolved_paths[@]}"
}

resolve_paths_from_html_raw() {
    local raw_html="$1"
    local -a html_candidates=()
    local -a resolved_paths=()

    [ -n "$raw_html" ] || return 1

    mapfile -t html_candidates < <(html_to_lines "$raw_html")
    [ "${#html_candidates[@]}" -gt 0 ] || return 1

    mapfile -t resolved_paths < <(collect_paths "${html_candidates[@]}")
    [ "${#resolved_paths[@]}" -gt 0 ] || return 1

    printf '%s\n' "${resolved_paths[@]}"
}

resolve_paths_from_dolphin_location_raw() {
    local base_dir="$1"
    local raw_plain="$2"
    local raw_html="$3"
    local -a location_candidates=()
    local -a resolved_paths=()

    mapfile -t location_candidates < <(resolve_dolphin_location_candidates "$base_dir" "$raw_plain" "$raw_html" || true)
    [ "${#location_candidates[@]}" -gt 0 ] || return 1

    mapfile -t resolved_paths < <(collect_paths "${location_candidates[@]}")
    [ "${#resolved_paths[@]}" -gt 0 ] || return 1

    printf '%s\n' "${resolved_paths[@]}"
}

wait_for_clipboard_paths() {
    local mode="$1"
    local base_dir="$2"
    local before_uri="$3"
    local before_plain="$4"
    local before_html="$5"
    local delay raw_uri raw_plain raw_html resolved_paths=""

    for delay in 0.01 0.01 0.015 0.015 0.02 0.02 0.03 0.03 0.04 0.05 0.06 0.08; do
        case "$mode" in
            dolphin-location)
                raw_plain="$(read_plain_text_raw)"
                if [ -n "$raw_plain" ] && [ "$raw_plain" != "$before_plain" ]; then
                    resolved_paths="$(resolve_paths_from_dolphin_location_raw "$base_dir" "$raw_plain" "" || true)"
                    [ -n "$resolved_paths" ] && printf '%s\n' "$resolved_paths" && return 0
                fi

                raw_html="$(read_html_raw)"
                if [ -n "$raw_html" ] && [ "$raw_html" != "$before_html" ]; then
                    resolved_paths="$(resolve_paths_from_dolphin_location_raw "$base_dir" "" "$raw_html" || true)"
                    [ -n "$resolved_paths" ] && printf '%s\n' "$resolved_paths" && return 0
                fi

                raw_uri="$(read_uri_list_raw)"
                if [ -n "$raw_uri" ] && [ "$raw_uri" != "$before_uri" ]; then
                    resolved_paths="$(resolve_paths_from_uri_raw "$raw_uri" || true)"
                    [ -n "$resolved_paths" ] && printf '%s\n' "$resolved_paths" && return 0
                fi
                ;;
            dolphin-copy)
                raw_uri="$(read_uri_list_raw)"
                if [ -n "$raw_uri" ] && [ "$raw_uri" != "$before_uri" ]; then
                    resolved_paths="$(resolve_paths_from_uri_raw "$raw_uri" || true)"
                    [ -n "$resolved_paths" ] && printf '%s\n' "$resolved_paths" && return 0
                fi

                raw_plain="$(read_plain_text_raw)"
                if [ -n "$raw_plain" ] && [ "$raw_plain" != "$before_plain" ]; then
                    resolved_paths="$(resolve_paths_from_plain_raw "$raw_plain" || true)"
                    [ -n "$resolved_paths" ] && printf '%s\n' "$resolved_paths" && return 0
                fi
                ;;
            generic)
                raw_uri="$(read_uri_list_raw)"
                if [ -n "$raw_uri" ] && [ "$raw_uri" != "$before_uri" ]; then
                    resolved_paths="$(resolve_paths_from_uri_raw "$raw_uri" || true)"
                    [ -n "$resolved_paths" ] && printf '%s\n' "$resolved_paths" && return 0
                fi

                raw_plain="$(read_plain_text_raw)"
                if [ -n "$raw_plain" ] && [ "$raw_plain" != "$before_plain" ]; then
                    resolved_paths="$(resolve_paths_from_plain_raw "$raw_plain" || true)"
                    [ -n "$resolved_paths" ] && printf '%s\n' "$resolved_paths" && return 0
                fi

                raw_html="$(read_html_raw)"
                if [ -n "$raw_html" ] && [ "$raw_html" != "$before_html" ]; then
                    resolved_paths="$(resolve_paths_from_html_raw "$raw_html" || true)"
                    [ -n "$resolved_paths" ] && printf '%s\n' "$resolved_paths" && return 0
                fi
                ;;
        esac

        sleep "$delay"
    done

    return 1
}

read_paths_from_dolphin() {
    local active_pid="$1"
    local service object_path state_dir current_dir
    local before_uri before_plain before_html copied_paths location_paths
    local started_us lookup_started_us copy_location_started_us edit_copy_started_us

    has_command busctl || return 1
    has_command wl-copy || return 1

    started_us="$(now_us)"
    lookup_started_us="$(now_us)"
    service="$(active_dolphin_service "$active_pid")" || return 1
    object_path="$(active_dolphin_object_path "$service")" || return 1
    timing_log get-file-path dolphin-lookup "duration_ms=$(elapsed_ms "$lookup_started_us")" "service=$service" "object=$object_path"

    state_dir="$(mktemp -d "${TMPDIR:-/tmp}/kanyrun-open.XXXXXX")"
    if ! save_clipboard_state "$state_dir"; then
        timing_log get-file-path dolphin-error "stage=save-clipboard"
        rm -rf "$state_dir"
        return 1
    fi

    current_dir="$(dolphin_window_directory "$service" "$object_path" || true)"

    before_uri="$(read_uri_list_raw)"
    before_plain="$(read_plain_text_raw)"
    before_html="$(read_html_raw)"

    copy_location_started_us="$(now_us)"
    if [ "$(busctl_bool_reply "$service" "$object_path" org.kde.KMainWindow activateAction s copy_location)" = "true" ]; then
        location_paths="$(wait_for_clipboard_paths dolphin-location "$current_dir" "$before_uri" "$before_plain" "$before_html" || true)"
        if [ -n "$location_paths" ]; then
            restore_clipboard_state "$state_dir"
            rm -rf "$state_dir"
            timing_log get-file-path dolphin-copy_location "duration_ms=$(elapsed_ms "$copy_location_started_us")" "result=hit"
            timing_log get-file-path dolphin-total "duration_ms=$(elapsed_ms "$started_us")" "result=hit"
            printf '%s\n' "$location_paths"
            return 0
        fi
    fi
    timing_log get-file-path dolphin-copy_location "duration_ms=$(elapsed_ms "$copy_location_started_us")" "result=miss"

    before_uri="$(read_uri_list_raw)"
    before_plain="$(read_plain_text_raw)"
    before_html=""

    edit_copy_started_us="$(now_us)"
    if [ "$(busctl_bool_reply "$service" "$object_path" org.kde.KMainWindow activateAction s edit_copy)" = "true" ]; then
        copied_paths="$(wait_for_clipboard_paths dolphin-copy "" "$before_uri" "$before_plain" "$before_html" || true)"
        if [ -n "$copied_paths" ]; then
            restore_clipboard_state "$state_dir"
            rm -rf "$state_dir"
            timing_log get-file-path dolphin-edit_copy "duration_ms=$(elapsed_ms "$edit_copy_started_us")" "result=hit"
            timing_log get-file-path dolphin-total "duration_ms=$(elapsed_ms "$started_us")" "result=hit"
            printf '%s\n' "$copied_paths"
            return 0
        fi
    fi
    timing_log get-file-path dolphin-edit_copy "duration_ms=$(elapsed_ms "$edit_copy_started_us")" "result=miss"

    restore_clipboard_state "$state_dir"
    rm -rf "$state_dir"
    timing_log get-file-path dolphin-total "duration_ms=$(elapsed_ms "$started_us")" "result=miss"
    return 1
}

read_paths_from_generic_copy() {
    local active_class="$1"
    local state_dir before_uri before_plain before_html copied_paths
    local started_us send_started_us

    has_command wl-copy || return 1
    has_command wl-paste || return 1
    has_command kdotool || return 1
    has_command dotool || has_command wtype || return 1

    started_us="$(now_us)"
    state_dir="$(mktemp -d "${TMPDIR:-/tmp}/kanyrun-open.XXXXXX")"
    if ! save_clipboard_state "$state_dir"; then
        timing_log get-file-path generic-error "stage=save-clipboard"
        rm -rf "$state_dir"
        return 1
    fi

    before_uri="$(read_uri_list_raw)"
    before_plain="$(read_plain_text_raw)"
    before_html="$(read_html_raw)"

    send_started_us="$(now_us)"
    if ! send_copy_shortcut "$active_class"; then
        restore_clipboard_state "$state_dir"
        rm -rf "$state_dir"
        timing_log get-file-path generic-send_copy "duration_ms=$(elapsed_ms "$send_started_us")" "result=failed"
        return 1
    fi
    timing_log get-file-path generic-send_copy "duration_ms=$(elapsed_ms "$send_started_us")" "result=ok"

    copied_paths="$(wait_for_clipboard_paths generic "" "$before_uri" "$before_plain" "$before_html" || true)"

    restore_clipboard_state "$state_dir"
    rm -rf "$state_dir"

    [ -n "$copied_paths" ] || {
        timing_log get-file-path generic-total "duration_ms=$(elapsed_ms "$started_us")" "result=miss"
        return 1
    }
    timing_log get-file-path generic-total "duration_ms=$(elapsed_ms "$started_us")" "result=hit"
    printf '%s\n' "$copied_paths"
}

format_block() {
    local title="$1"
    local value="$2"

    if [ -n "$value" ]; then
        printf '=== %s ===\n%s\n\n' "$title" "$value"
    else
        printf '=== %s ===\n<empty>\n\n' "$title"
    fi
}

join_args() {
    if [ "$#" -eq 0 ]; then
        return
    fi

    local arg
    for arg in "$@"; do
        printf '%s\n' "$arg"
    done
}

main() {
    local quiet_first_path="false"
    local uri_raw=""
    local clipboard_raw=""
    local primary_raw=""
    local argv_raw=""
    local candidate_report=""
    local resolved_report=""
    local source=""
    local selected_count="0"
    local resolved_count="0"
    local first_path=""
    local active_window_class=""
    local active_window_pid=""
    local report=""
    local -a raw_paths=()
    local -a resolved_paths=()
    local -a dolphin_paths=()
    local -a generic_paths=()
    local started_us=""
    local active_started_us=""
    local provider_started_us=""
    local fallback_started_us=""
    local resolve_started_us=""

    started_us="$(now_us)"
    timing_log get-file-path start "argv_count=$#"

    if [ "$#" -gt 0 ] && [ "$1" = "--first-path" ]; then
        quiet_first_path="true"
        shift
    fi

    if [ "$#" -gt 0 ]; then
        argv_raw="$(join_args "$@")"
        source="argv"
        mapfile -t raw_paths < <(printf '%s\n' "$@")
        timing_log get-file-path argv-input "count=${#raw_paths[@]}"
    else
        if has_command wl-paste; then
            active_started_us="$(now_us)"
            active_window_class="$(get_active_window_class || true)"
            active_window_pid="$(get_active_window_pid || true)"
            timing_log get-file-path active-window "duration_ms=$(elapsed_ms "$active_started_us")" "class=${active_window_class:-<empty>}" "pid=${active_window_pid:-<empty>}"

            if window_class_is_dolphin "$active_window_class"; then
                provider_started_us="$(now_us)"
                mapfile -t dolphin_paths < <(read_paths_from_dolphin "$active_window_pid" || true)
                timing_log get-file-path provider "name=dolphin" "duration_ms=$(elapsed_ms "$provider_started_us")" "count=${#dolphin_paths[@]}"
                if [ "${#dolphin_paths[@]}" -gt 0 ]; then
                    source="dolphin selection"
                    raw_paths=("${dolphin_paths[@]}")
                fi
            else
                provider_started_us="$(now_us)"
                mapfile -t generic_paths < <(read_paths_from_generic_copy "$active_window_class" || true)
                timing_log get-file-path provider "name=generic" "duration_ms=$(elapsed_ms "$provider_started_us")" "count=${#generic_paths[@]}"
                if [ "${#generic_paths[@]}" -gt 0 ]; then
                    source="active-window copy"
                    raw_paths=("${generic_paths[@]}")
                fi
            fi

            if [ "${#raw_paths[@]}" -eq 0 ]; then
                fallback_started_us="$(now_us)"
                uri_raw="$(read_uri_list_raw)"
                clipboard_raw="$(read_plain_text_raw)"
                primary_raw="$(read_primary_text_raw)"

                if [ -n "$uri_raw" ]; then
                    source="clipboard text/uri-list"
                    mapfile -t raw_paths < <(read_uri_candidates "$uri_raw")
                elif [ -n "$clipboard_raw" ]; then
                    source="clipboard text/plain"
                    mapfile -t raw_paths < <(split_nonempty_lines "$clipboard_raw")
                elif [ -n "$primary_raw" ]; then
                    source="primary text/plain"
                    mapfile -t raw_paths < <(split_nonempty_lines "$primary_raw")
                else
                    source="none"
                fi
                timing_log get-file-path fallback "duration_ms=$(elapsed_ms "$fallback_started_us")" "source=$source" "count=${#raw_paths[@]}"
            fi
        else
            source="wl-paste unavailable"
            timing_log get-file-path error "stage=wl-paste-unavailable"
        fi
    fi

    selected_count="${#raw_paths[@]}"
    if [ "${#raw_paths[@]}" -gt 0 ]; then
        resolve_started_us="$(now_us)"
        candidate_report="$(printf '%s\n' "${raw_paths[@]}")"
        mapfile -t resolved_paths < <(collect_paths "${raw_paths[@]}")
        timing_log get-file-path resolve "duration_ms=$(elapsed_ms "$resolve_started_us")" "raw_count=${#raw_paths[@]}" "resolved_count=${#resolved_paths[@]}"
    fi

    resolved_count="${#resolved_paths[@]}"
    if [ "${#resolved_paths[@]}" -gt 0 ]; then
        first_path="${resolved_paths[0]}"
        resolved_report="$(printf '%s\n' "${resolved_paths[@]}")"
    fi

    timing_log get-file-path result "duration_ms=$(elapsed_ms "$started_us")" "source=$source" "selected_count=$selected_count" "resolved_count=$resolved_count" "first_path=${first_path:-<empty>}"

    if [ "$quiet_first_path" = "true" ]; then
        [ -n "$first_path" ] || return 1
        printf '%s\n' "$first_path"
        return 0
    fi

    report="$(
        format_block "source" "$source"
        format_block "active window class" "$active_window_class"
        format_block "active window pid" "$active_window_pid"
        format_block "selected count" "$selected_count"
        format_block "resolved count" "$resolved_count"
        format_block "first path (default)" "$first_path"
        format_block "argv" "$argv_raw"
        format_block "clipboard uri-list raw" "$uri_raw"
        format_block "clipboard text raw" "$clipboard_raw"
        format_block "primary text raw" "$primary_raw"
        format_block "candidates" "$candidate_report"
        format_block "resolved paths" "$resolved_report"
    )"

    show_output "$report"
}

main "$@"
