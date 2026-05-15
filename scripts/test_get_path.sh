#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

fail() {
    printf 'FAIL: %s\n' "$1" >&2
    exit 1
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
            printf 'file://%s' "$sample_file" > "\$GET_PATH_FAKE_URI"
            : > "\$GET_PATH_FAKE_PLAIN"
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
    [ -s "$GET_PATH_FAKE_URI" ] && printf 'text/uri-list\n'
    [ -s "$GET_PATH_FAKE_PLAIN" ] && printf 'text/plain\n'
    exit 0
fi

type="text/plain"
while [ "$#" -gt 0 ]; do
    case "$1" in
        --type) shift; type="${1:-text/plain}" ;;
    esac
    shift || true
done

if [ "$type" = "text/uri-list" ]; then
    [ -f "$GET_PATH_FAKE_URI" ] && cat "$GET_PATH_FAKE_URI"
elif [ "$type" = "text/plain" ]; then
    [ -f "$GET_PATH_FAKE_PLAIN" ] && cat "$GET_PATH_FAKE_PLAIN"
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
    : > "$GET_PATH_FAKE_URI"
    : > "$GET_PATH_FAKE_PLAIN"
    exit 0
fi

if [ "$type" = "text/uri-list" ]; then
    cat > "$GET_PATH_FAKE_URI"
else
    cat > "$GET_PATH_FAKE_PLAIN"
fi
EOF
    chmod +x "$dir/wl-copy"
}

run_get_path() {
    cargo run --quiet --manifest-path "$ROOT/Cargo.toml" --bin get-path -- "$@"
}

run_explicit_path_case() {
    local tmpdir sample_file output
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' RETURN
    sample_file="$tmpdir/sample.txt"
    printf 'sample' > "$sample_file"

    output="$(run_get_path -- "$sample_file")"
    [ "$output" = "$sample_file" ] || fail 'explicit path output mismatch'
}

run_dolphin_edit_copy_case() {
    local tmpdir sample_file output
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' RETURN
    sample_file="$tmpdir/from-dolphin.txt"
    printf 'sample' > "$sample_file"
    : > "$tmpdir/uri"
    : > "$tmpdir/plain"
    make_fake_busctl "$tmpdir" copy-uri "$sample_file"
    make_fake_wl_tools "$tmpdir"

    output="$(PATH="$tmpdir:$PATH" GET_PATH_FAKE_URI="$tmpdir/uri" GET_PATH_FAKE_PLAIN="$tmpdir/plain" run_get_path)"
    [ "$output" = "$sample_file" ] || fail 'dolphin edit_copy output mismatch'
}

run_stale_clipboard_case() {
    local tmpdir stale_file output status
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' RETURN
    stale_file="$tmpdir/stale.txt"
    printf 'stale' > "$stale_file"
    printf 'file://%s' "$stale_file" > "$tmpdir/uri"
    : > "$tmpdir/plain"
    make_fake_busctl "$tmpdir" stale "$stale_file"
    make_fake_wl_tools "$tmpdir"

    set +e
    output="$(PATH="$tmpdir:$PATH" GET_PATH_FAKE_URI="$tmpdir/uri" GET_PATH_FAKE_PLAIN="$tmpdir/plain" GET_PATH_WAIT_ATTEMPTS=1 GET_PATH_WAIT_DELAY_MS=0 run_get_path 2>&1)"
    status=$?
    set -e

    [ "$status" -ne 0 ] || fail 'stale clipboard path was accepted'
    grep -F -- 'no fresh paths copied from Dolphin' <<< "$output" >/dev/null || fail 'stale clipboard error mismatch'
}

run_explicit_path_case
run_dolphin_edit_copy_case
run_stale_clipboard_case
printf 'test_get_path.sh: ok\n'
