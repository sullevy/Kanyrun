#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT="$ROOT/kanyrun-open.sh"

fail() {
    printf 'FAIL: %s\n' "$1" >&2
    exit 1
}

make_fake_kanyrun() {
    local dir="$1"

    cat > "$dir/kanyrun" <<'EOF'
#!/usr/bin/env bash
printf 'ARGS:'
for arg in "$@"; do
    printf ' <%s>' "$arg"
done
printf '\nFIRST=%s\n' "${KANYRUN_SELECTED_FIRST:-}"
printf 'COUNT=%s\n' "${KANYRUN_SELECTED_COUNT:-}"
printf 'PATHS=%s\n' "${KANYRUN_SELECTED_PATHS:-}"
printf 'DIR=%s\n' "${KANYRUN_SELECTED_DIR:-}"
printf 'NAME=%s\n' "${KANYRUN_SELECTED_NAME:-}"
printf 'STEMS=%s\n' "${KANYRUN_SELECTED_STEMS:-}"
EOF
    chmod +x "$dir/kanyrun"
}

make_fake_copyq() {
    local dir="$1"

    cat > "$dir/copyq" <<'EOF'
#!/usr/bin/env bash
printf 'copyq called: %s\n' "$*" >> "$KANYRUN_TEST_COPYQ_LOG"
EOF
    chmod +x "$dir/copyq"
}

make_fake_wl_copy() {
    local dir="$1"

    cat > "$dir/wl-copy" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
    chmod +x "$dir/wl-copy"
}

make_fake_wl_paste() {
    local dir="$1"

    cat > "$dir/wl-paste" <<'EOF'
#!/usr/bin/env bash
type="text/plain"
while [ "$#" -gt 0 ]; do
    case "$1" in
        --type) shift; type="${1:-text/plain}" ;;
    esac
    shift || true
done

if [ "$type" = "text/uri-list" ]; then
    [ -f "$KANYRUN_TEST_URI" ] && cat "$KANYRUN_TEST_URI"
elif [ "$type" = "text/plain" ]; then
    [ -f "$KANYRUN_TEST_PLAIN" ] && cat "$KANYRUN_TEST_PLAIN"
fi
EOF
    chmod +x "$dir/wl-paste"
}

make_fake_dotoolc() {
    local dir="$1"

    cat > "$dir/dotoolc" <<'EOF'
#!/usr/bin/env bash
cat >/dev/null
exit 0
EOF
    chmod +x "$dir/dotoolc"
}

make_fake_noop_copy_tools() {
    local dir="$1"

    for tool in dotoolc dotool wtype kdotool; do
        cat > "$dir/$tool" <<'EOF'
#!/usr/bin/env bash
exit 1
EOF
        chmod +x "$dir/$tool"
    done
}

run_explicit_path_case() {
    local tmpdir sample_file output copyq_log timing_log
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' RETURN
    sample_file="$tmpdir/sample.txt"
    copyq_log="$tmpdir/copyq.log"
    timing_log="$tmpdir/timing.log"
    printf 'sample' > "$sample_file"
    make_fake_kanyrun "$tmpdir"
    make_fake_copyq "$tmpdir"

    output="$(PATH="$tmpdir:$PATH" XDG_DATA_HOME="$tmpdir/xdg" KANYRUN_TEST_COPYQ_LOG="$copyq_log" KANYRUN_DISMISS_EXISTING=0 KANYRUN_OPEN_TIMING_LOG="$timing_log" "$SCRIPT" --debug "$sample_file")"
    grep -F -- "--files> <$sample_file" <<< "$output" >/dev/null || fail 'explicit path was not passed to kanyrun --files'
    grep -F -- "COUNT=1" <<< "$output" >/dev/null || fail 'explicit path did not export selected count'
    grep -F -- "DIR=$tmpdir" <<< "$output" >/dev/null || fail 'explicit path did not export selected dir'
    grep -F -- "NAME=sample.txt" <<< "$output" >/dev/null || fail 'explicit path did not export selected name'
    grep -F -- "STEMS=sample.txt" <<< "$output" >/dev/null || fail 'explicit path did not export selected stems'
    [ ! -s "$copyq_log" ] || fail 'script should not call copyq'
    grep -F -- "stage=collect_paths" "$timing_log" >/dev/null || fail 'timing log missing collect_paths stage'
    grep -F -- "stage=exec_menu" "$timing_log" >/dev/null || fail 'timing log missing exec_menu stage'
    grep -F -- "argc=1" "$timing_log" >/dev/null || fail 'timing log missing argc'
}

run_default_timing_disabled_case() {
    local tmpdir sample_file output copyq_log timing_log
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' RETURN
    sample_file="$tmpdir/sample.txt"
    copyq_log="$tmpdir/copyq.log"
    timing_log="$tmpdir/timing.log"
    printf 'sample' > "$sample_file"
    make_fake_kanyrun "$tmpdir"
    make_fake_copyq "$tmpdir"

    output="$(PATH="$tmpdir:$PATH" XDG_DATA_HOME="$tmpdir/xdg" KANYRUN_TEST_COPYQ_LOG="$copyq_log" KANYRUN_DISMISS_EXISTING=0 KANYRUN_OPEN_TIMING=1 KANYRUN_OPEN_TIMING_LOG="$timing_log" "$SCRIPT" "$sample_file")"
    grep -F -- "--files> <$sample_file" <<< "$output" >/dev/null || fail 'explicit path was not passed with timing disabled'
    [ ! -s "$timing_log" ] || fail 'timing log should be disabled by default'
}

run_debug_flag_enables_timing_case() {
    local tmpdir sample_file output copyq_log timing_log
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' RETURN
    sample_file="$tmpdir/sample.txt"
    copyq_log="$tmpdir/copyq.log"
    timing_log="$tmpdir/timing.log"
    printf 'sample' > "$sample_file"
    make_fake_kanyrun "$tmpdir"
    make_fake_copyq "$tmpdir"

    output="$(PATH="$tmpdir:$PATH" XDG_DATA_HOME="$tmpdir/xdg" KANYRUN_TEST_COPYQ_LOG="$copyq_log" KANYRUN_DISMISS_EXISTING=0 KANYRUN_OPEN_TIMING_LOG="$timing_log" "$SCRIPT" --debug "$sample_file")"
    grep -F -- "ARGS: <--debug>" <<< "$output" >/dev/null || fail 'debug flag was not passed to kanyrun'
    grep -F -- "--files> <$sample_file" <<< "$output" >/dev/null || fail 'debug path was not passed to kanyrun --files'
    grep -F -- "stage=exec_menu" "$timing_log" >/dev/null || fail 'debug flag did not enable timing log'
}

run_copy_shortcut_uri_case() {
    local tmpdir sample_file output copyq_log timing_log uri_file plain_file
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' RETURN
    sample_file="$tmpdir/from search.pdf"
    copyq_log="$tmpdir/copyq.log"
    timing_log="$tmpdir/timing.log"
    uri_file="$tmpdir/uri"
    plain_file="$tmpdir/plain"
    printf 'sample' > "$sample_file"
    printf 'file://%s\n' "${sample_file// /%20}" > "$uri_file"
    : > "$plain_file"
    make_fake_kanyrun "$tmpdir"
    make_fake_copyq "$tmpdir"
    make_fake_wl_copy "$tmpdir"
    make_fake_wl_paste "$tmpdir"
    make_fake_dotoolc "$tmpdir"

    output="$(PATH="$tmpdir:$PATH" XDG_DATA_HOME="$tmpdir/xdg" KANYRUN_TEST_COPYQ_LOG="$copyq_log" KANYRUN_TEST_URI="$uri_file" KANYRUN_TEST_PLAIN="$plain_file" KANYRUN_DISMISS_EXISTING=0 KANYRUN_OPEN_TIMING_LOG="$timing_log" "$SCRIPT" --debug)"

    grep -F -- "--files> <file://$tmpdir/from%20search.pdf>" <<< "$output" >/dev/null || fail 'copied URI was not passed to kanyrun --files'
    grep -F -- "COUNT=1" <<< "$output" >/dev/null || fail 'copied URI did not export selected count'
    [ ! -s "$copyq_log" ] || fail 'copy shortcut path should not call copyq'
    grep -F -- "stage=send_copy" "$timing_log" >/dev/null || fail 'timing log missing send_copy stage'
    grep -F -- "source=uri-list" "$timing_log" >/dev/null || fail 'timing log missing uri-list source'
}

run_context_menu_fallback_case() {
    local tmpdir output status copyq_log timing_log uri_file plain_file
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' RETURN
    copyq_log="$tmpdir/copyq.log"
    timing_log="$tmpdir/timing.log"
    uri_file="$tmpdir/uri"
    plain_file="$tmpdir/plain"
    : > "$uri_file"
    : > "$plain_file"

    make_fake_kanyrun "$tmpdir"
    make_fake_copyq "$tmpdir"
    make_fake_wl_copy "$tmpdir"
    make_fake_wl_paste "$tmpdir"
    make_fake_noop_copy_tools "$tmpdir"

    set +e
    output="$(PATH="$tmpdir:$PATH" XDG_DATA_HOME="$tmpdir/xdg" KANYRUN_TEST_COPYQ_LOG="$copyq_log" KANYRUN_TEST_URI="$uri_file" KANYRUN_TEST_PLAIN="$plain_file" KANYRUN_DISMISS_EXISTING=0 KANYRUN_OPEN_TIMING_LOG="$timing_log" "$SCRIPT" --debug 2>&1)"
    status=$?
    set -e

    [ "$status" -eq 0 ] || fail 'missing paths did not fall back to default menu'
    grep -F -- "ARGS: <--debug> <--daemon-idle-timeout> <300> <--ui-idle-timeout> <15> <--menu> <--from-primary>" <<< "$output" >/dev/null || fail 'context menu fallback arguments mismatch'
    grep -F -- "FIRST=" <<< "$output" >/dev/null || fail 'context menu fallback should not export FIRST'
    [ ! -s "$copyq_log" ] || fail 'fallback path should not call copyq'
    grep -F -- "stage=exec_context_menu" "$timing_log" >/dev/null || fail 'timing log missing exec_context_menu stage'
    grep -F -- "argc=0" "$timing_log" >/dev/null || fail 'fallback timing log missing argc'
}

run_explicit_path_case
run_default_timing_disabled_case
run_debug_flag_enables_timing_case
run_copy_shortcut_uri_case
run_context_menu_fallback_case
printf 'test_kanyrun_open.sh: ok\n'
