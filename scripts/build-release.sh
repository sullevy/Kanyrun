#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RELEASE_DIR="$ROOT_DIR/release"
CONFIG_DIR="$RELEASE_DIR/config"
SAMPLES_DIR="$RELEASE_DIR/samples"
UI_SOURCE_DIR="$ROOT_DIR/ui/kanyrun-ui"
UI_BUILD_DIR="$UI_SOURCE_DIR/build-release"
RUST_BINARY="$ROOT_DIR/target/release/kanyrun"
UI_BINARY="$UI_BUILD_DIR/kanyrun-ui"
KANYRUN_OPEN_SOURCE="${KANYRUN_OPEN_SOURCE:-$ROOT_DIR/kanyrun-open.sh}"
LEGACY_GET_FILE_PATH_SOURCE="$RELEASE_DIR/get-file-path.sh"
LEGACY_GET_FILE_PATH_BACKUP=""

printf 'Building Rust release binary...\n'
cargo build --release --manifest-path "$ROOT_DIR/Cargo.toml"

if [ ! -f "$UI_BUILD_DIR/CMakeCache.txt" ]; then
    printf 'Configuring Qt UI release build...\n'
    cmake -S "$UI_SOURCE_DIR" -B "$UI_BUILD_DIR" -DCMAKE_BUILD_TYPE=Release
fi

printf 'Building Qt UI release binary...\n'
cmake --build "$UI_BUILD_DIR"

if [ ! -x "$RUST_BINARY" ]; then
    printf 'Rust release binary is missing after build: %s\n' "$RUST_BINARY" >&2
    exit 1
fi

if [ ! -x "$UI_BINARY" ]; then
    printf 'Qt UI release binary is missing after build: %s\n' "$UI_BINARY" >&2
    exit 1
fi

if [ -f "$LEGACY_GET_FILE_PATH_SOURCE" ]; then
    LEGACY_GET_FILE_PATH_BACKUP="$(mktemp "${TMPDIR:-/tmp}/kanyrun-get-file-path.XXXXXX")"
    install -m 0755 "$LEGACY_GET_FILE_PATH_SOURCE" "$LEGACY_GET_FILE_PATH_BACKUP"
fi

rm -rf "$RELEASE_DIR"
mkdir -p "$CONFIG_DIR" "$SAMPLES_DIR"

install -m 0755 "$RUST_BINARY" "$RELEASE_DIR/kanyrun"
install -m 0755 "$UI_BINARY" "$RELEASE_DIR/kanyrun-ui"
install -m 0755 "$ROOT_DIR/install.sh" "$RELEASE_DIR/install.sh"
install -m 0755 "$ROOT_DIR/uninstall.sh" "$RELEASE_DIR/uninstall.sh"
install -m 0644 "$ROOT_DIR/LICENSE" "$RELEASE_DIR/LICENSE"
install -m 0644 "$ROOT_DIR/USAGE.md" "$RELEASE_DIR/USAGE.md"
if [ ! -x "$KANYRUN_OPEN_SOURCE" ]; then
    printf 'kanyrun-open source script is missing: %s\n' "$KANYRUN_OPEN_SOURCE" >&2
    exit 1
fi
install -m 0755 "$KANYRUN_OPEN_SOURCE" "$RELEASE_DIR/kanyrun-open.sh"
if [ -n "$LEGACY_GET_FILE_PATH_BACKUP" ]; then
    install -m 0755 "$LEGACY_GET_FILE_PATH_BACKUP" "$RELEASE_DIR/get-file-path.sh"
    rm -f "$LEGACY_GET_FILE_PATH_BACKUP"
fi
install -m 0644 "$ROOT_DIR/使用说明.md" "$RELEASE_DIR/使用说明.md"
install -m 0644 "$ROOT_DIR/assets/config/config.toml" "$CONFIG_DIR/config.toml"
install -m 0644 "$ROOT_DIR/assets/config/menu.ini" "$CONFIG_DIR/menu.ini"
install -m 0644 "$ROOT_DIR/assets/config/config.sample.toml" "$CONFIG_DIR/config.sample.toml"
install -m 0644 "$ROOT_DIR/assets/config/menu_sample.ini" "$CONFIG_DIR/menu_sample.ini"
install -m 0644 "$ROOT_DIR/assets/config/config.sample.toml" "$SAMPLES_DIR/config.sample.toml"
install -m 0644 "$ROOT_DIR/assets/config/menu_sample.ini" "$SAMPLES_DIR/menu_sample.ini"

printf 'Release bundle synchronized at %s\n' "$RELEASE_DIR"
