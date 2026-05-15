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
EOF
    chmod +x "$dir/kanyrun"
}

make_fake_busctl() {
    local dir="$1"
    local mode="$2"
    local sample_file="$3"

    cat > "$dir/busctl" <<EOF
#!/usr/bin/env bash
case "\$*" in
    *"--acquired list"*)
        printf 'org.kde.dolphin-1234 1234 dolphin sullevy :1.1 user@1000.service - -\\n'
        ;;
    *"--list tree org.kde.dolphin-1234"*)
        printf '/dolphin/Dolphin_1\\n'
        ;;
    *"/dolphin/Dolphin_1 org.kde.dolphin.MainWindow isActiveWindow"*)
        printf 'b true\\n'
        ;;
    *"/dolphin/Dolphin_1/actions/edit_copy org.qtproject.Qt.QAction enabled"*)
        printf 'b true\\n'
        ;;
    *"/dolphin/Dolphin_1/actions/copy_location org.qtproject.Qt.QAction enabled"*)
        printf 'b false\\n'
        ;;
    *"/dolphin/Dolphin_1/actions/edit_copy org.qtproject.Qt.QAction trigger"*)
        if [ "$mode" = "copy-uri" ]; then
            printf 'file://%s' "$sample_file" > "\$KANYRUN_FAKE_URI"
            : > "\$KANYRUN_FAKE_PLAIN"
        fi
        ;;
    *)
        exit 1
        ;;
esac
EOF
    chmod +x "$dir/busctl"
}

make_fake_wl_tools() {
    local dir="$1"

    cat > "$dir/wl-paste" <<'EOF'
#!/usr/bin/env bash
if [ "${1:-}" = "--list-types" ]; then
    [ -s "$KANYRUN_FAKE_URI" ] && printf 'text/uri-list\n'
    [ -s "$KANYRUN_FAKE_PLAIN" ] && printf 'text/plain\n'
    exit 0
fi

primary=false
type="text/plain"
while [ "$#" -gt 0 ]; do
    case "$1" in
        --primary) primary=true ;;
        --type) shift; type="${1:-text/plain}" ;;
    esac
    shift || true
done

if [ "$type" = "text/uri-list" ]; then
    [ -f "$KANYRUN_FAKE_URI" ] && cat "$KANYRUN_FAKE_URI"
elif [ "$type" = "text/plain" ]; then
    [ -f "$KANYRUN_FAKE_PLAIN" ] && cat "$KANYRUN_FAKE_PLAIN"
fi
EOF
    chmod +x "$dir/wl-paste"

    cat > "$dir/wl-copy" <<'EOF'
#!/usr/bin/env bash
type="text/plain"
clear=false
while [ "$#" -gt 0 ]; do
    case "$1" in
        --clear) clear=true ;;
        --type) shift; type="${1:-text/plain}" ;;
    esac
    shift || true
done

if [ "$clear" = true ]; then
    : > "$KANYRUN_FAKE_URI"
    : > "$KANYRUN_FAKE_PLAIN"
    exit 0
fi

if [ "$type" = "text/uri-list" ]; then
    cat > "$KANYRUN_FAKE_URI"
else
    cat > "$KANYRUN_FAKE_PLAIN"
fi
EOF
    chmod +x "$dir/wl-copy"
}

build_get_path() {
    cargo build --quiet --manifest-path "$ROOT/Cargo.toml" --bin get-path
}

run_explicit_path_case() {
    local tmpdir sample_file output
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' RETURN
    sample_file="$tmpdir/sample.txt"
    printf 'sample' > "$sample_file"
    make_fake_kanyrun "$tmpdir"

    output="$(PATH="$tmpdir:$ROOT/target/debug:$PATH" XDG_DATA_HOME="$tmpdir/xdg" "$SCRIPT" "$sample_file")"
    grep -F -- "--files> <$sample_file" <<< "$output" >/dev/null || fail 'explicit path was not passed to kanyrun --files'
    grep -F -- "COUNT=1" <<< "$output" >/dev/null || fail 'explicit path did not export selected count'
}

run_dolphin_uri_case() {
    local tmpdir sample_file output
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' RETURN
    sample_file="$tmpdir/from-dolphin.txt"
    printf 'sample' > "$sample_file"
    : > "$tmpdir/uri"
    : > "$tmpdir/plain"
    make_fake_kanyrun "$tmpdir"
    make_fake_busctl "$tmpdir" copy-uri "$sample_file"
    make_fake_wl_tools "$tmpdir"

    output="$(PATH="$tmpdir:$ROOT/target/debug:$PATH" XDG_DATA_HOME="$tmpdir/xdg" KANYRUN_FAKE_URI="$tmpdir/uri" KANYRUN_FAKE_PLAIN="$tmpdir/plain" "$SCRIPT")"
    grep -F -- "--files> <$sample_file" <<< "$output" >/dev/null || fail 'dolphin uri-list copy was not used'
    grep -F -- "COUNT=1" <<< "$output" >/dev/null || fail 'dolphin uri-list copy did not export selected count'
}

run_stale_clipboard_case() {
    local tmpdir stale_file output status
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' RETURN
    stale_file="$tmpdir/stale.txt"
    printf 'stale' > "$stale_file"
    printf 'file://%s' "$stale_file" > "$tmpdir/uri"
    : > "$tmpdir/plain"
    make_fake_kanyrun "$tmpdir"
    make_fake_busctl "$tmpdir" stale "$stale_file"
    make_fake_wl_tools "$tmpdir"

    set +e
    output="$(PATH="$tmpdir:$ROOT/target/debug:$PATH" XDG_DATA_HOME="$tmpdir/xdg" KANYRUN_FAKE_URI="$tmpdir/uri" KANYRUN_FAKE_PLAIN="$tmpdir/plain" GET_PATH_WAIT_ATTEMPTS=1 GET_PATH_WAIT_DELAY_MS=0 "$SCRIPT" 2>&1)"
    status=$?
    set -e

    [ "$status" -ne 0 ] || fail 'stale clipboard path was accepted as a fresh dolphin copy'
    grep -F -- 'No valid file or directory paths were found.' <<< "$output" >/dev/null || fail 'stale clipboard case did not report missing fresh paths'
}

build_get_path
run_explicit_path_case
run_dolphin_uri_case
run_stale_clipboard_case
printf 'test_kanyrun_open.sh: ok\n'
