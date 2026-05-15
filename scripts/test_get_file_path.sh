#!/usr/bin/env bash
set -euo pipefail

SCRIPT="/home/sullevy/Linux_Apps/scripts/get-file-path.sh"

assert_contains() {
    local haystack="$1"
    local needle="$2"

    case "$haystack" in
        *"$needle"*) ;;
        *)
            printf 'expected output to contain: %s\n' "$needle" >&2
            printf 'actual output:\n%s\n' "$haystack" >&2
            return 1
            ;;
    esac
}

run_dolphin_selection_case() {
    local tmpdir fakebin statefile sample_file output

    tmpdir="$(mktemp -d)"
    fakebin="$tmpdir/bin"
    statefile="$tmpdir/state"
    sample_file="$tmpdir/selected file.txt"
    mkdir -p "$fakebin"
    : > "$statefile"
    : > "$sample_file"
    export TEST_STATEFILE="$statefile"
    export TEST_SAMPLE_FILE="$sample_file"

    cat > "$fakebin/busctl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
statefile="$TEST_STATEFILE"
sample_file="$TEST_SAMPLE_FILE"

case "$*" in
    *"--acquired list"*)
        printf 'org.kde.dolphin-1234 1234 dolphin sullevy :1.1 user@1000.service - -\n'
        ;;
    *"--list tree org.kde.dolphin-1234"*)
        printf '/dolphin/Dolphin_1\n'
        ;;
    *"org.kde.dolphin.MainWindow isActiveWindow"*)
        printf 'b true\n'
        ;;
    *"org.kde.KMainWindow actionIsEnabled s edit_copy"*)
        printf 'b true\n'
        ;;
    *"org.kde.KMainWindow activateAction s edit_copy"*)
        printf 'file://%s\n' "$sample_file" > "$statefile.uri"
        printf 'b true\n'
        ;;
    *)
        exit 1
        ;;
esac
EOF

    cat > "$fakebin/wl-copy" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
statefile="$TEST_STATEFILE"

if [ "${1:-}" = "--clear" ]; then
    : > "$statefile.clip"
    exit 0
fi

cat > "$statefile.clip"
EOF

    cat > "$fakebin/wl-paste" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
statefile="$TEST_STATEFILE"

case "$*" in
    *"--list-types"*)
        printf 'text/plain\n'
        ;;
    *"--type text/uri-list"*)
        [ -f "$statefile.uri" ] && cat "$statefile.uri"
        ;;
    *"--primary --type text/plain"*)
        exit 0
        ;;
    *"--type text/plain"*)
        [ -f "$statefile.clip" ] && cat "$statefile.clip"
        ;;
    *)
        exit 0
        ;;
esac
EOF

    cat > "$fakebin/kdialog" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "${4:-}"
EOF

    cat > "$fakebin/sleep" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF

    chmod +x "$fakebin/busctl" "$fakebin/wl-copy" "$fakebin/wl-paste" "$fakebin/kdialog" "$fakebin/sleep"

    output="$(PATH="$fakebin:$PATH" "$SCRIPT")"

    assert_contains "$output" "=== resolved paths ==="
    assert_contains "$output" "$sample_file"

    rm -rf "$tmpdir"
}

run_dolphin_selection_case
printf 'ok\n'
