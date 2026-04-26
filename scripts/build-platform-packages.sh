#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PACKAGE_ROOT="$ROOT_DIR/ts/npm"
MAX_BYTES="${TALON_MAX_BINARY_BYTES:-31457280}"
SKIP_SMOKE="${TALON_SKIP_SMOKE:-0}"
REQUIRE_SMOKE="${TALON_REQUIRE_TARGET_SMOKE:-0}"
VERSION="$(cargo pkgid -p talon-cli --locked | sed 's/.*#//')"
TARGET_DIR="$(
  cargo metadata --format-version 1 --no-deps |
    sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p'
)"

targets=(
  "darwin-arm64|aarch64-apple-darwin|@seanmozeik/talon-darwin-arm64|darwin|arm64|talon"
  "darwin-x64|x86_64-apple-darwin|@seanmozeik/talon-darwin-x64|darwin|x64|talon"
  "linux-arm64|aarch64-unknown-linux-gnu|@seanmozeik/talon-linux-arm64|linux|arm64|talon"
  "linux-x64|x86_64-unknown-linux-gnu|@seanmozeik/talon-linux-x64|linux|x64|talon"
  "win32-x64|x86_64-pc-windows-gnu|@seanmozeik/talon-win32-x64|win32|x64|talon.exe"
)

host_os() {
  case "$(uname -s)" in
    Darwin) printf 'darwin' ;;
    Linux) printf 'linux' ;;
    *) printf 'unknown' ;;
  esac
}

host_arch() {
  case "$(uname -m)" in
    arm64 | aarch64) printf 'arm64' ;;
    x86_64 | amd64) printf 'x64' ;;
    *) printf 'unknown' ;;
  esac
}

write_package_json() {
  local package_dir="$1"
  local package_name="$2"
  local os="$3"
  local cpu="$4"

  printf '%s\n' \
    '{' \
    "  \"name\": \"$package_name\"," \
    "  \"version\": \"$VERSION\"," \
    '  "private": false,' \
    '  "description": "Prebuilt Talon binary.",' \
    '  "license": "MIT OR Apache-2.0",' \
    '  "repository": {' \
    '    "type": "git",' \
    '    "url": "git+https://github.com/seanmozeik/talon.git"' \
    '  },' \
    "  \"os\": [\"$os\"]," \
    "  \"cpu\": [\"$cpu\"]," \
    '  "files": [' \
    '    "bin/talon"' \
    '  ],' \
    '  "bin": {' \
    '    "talon": "bin/talon"' \
    '  }' \
    '}' > "$package_dir/package.json"
}

smoke_version() {
  local label="$1"
  local package_binary="$2"
  local build_binary="$3"
  local os="$4"
  local cpu="$5"

  if [[ "$SKIP_SMOKE" == "1" ]]; then
    printf 'smoke skipped for %s because TALON_SKIP_SMOKE=1\n' "$label"
    return 0
  fi

  if [[ "$(host_os)" == "$os" && "$(host_arch)" == "$cpu" ]]; then
    "$package_binary" --version
    return 0
  fi

  if [[ "$os" == "linux" && "$cpu" == "arm64" ]] && command -v qemu-aarch64 >/dev/null 2>&1; then
    qemu-aarch64 "$package_binary" --version
    return 0
  fi

  if [[ "$os" == "win32" && "$cpu" == "x64" ]] && command -v wine >/dev/null 2>&1; then
    wine "$build_binary" --version
    return 0
  fi

  if [[ "$REQUIRE_SMOKE" == "1" ]]; then
    printf 'no target runtime available to smoke %s\n' "$label" >&2
    return 1
  fi

  printf 'smoke skipped for %s; run on target OS/arch or install qemu/wine\n' "$label"
}

mkdir -p "$PACKAGE_ROOT"

for target in "${targets[@]}"; do
  IFS='|' read -r label triple package_name os cpu binary_name <<< "$target"
  printf '\n==> building %s (%s)\n' "$label" "$triple"

  cargo zigbuild -p talon-cli --release --target "$triple" --locked

  build_binary="$TARGET_DIR/$triple/release/$binary_name"
  if [[ ! -f "$build_binary" ]]; then
    printf 'expected build output missing: %s\n' "$build_binary" >&2
    exit 1
  fi

  package_dir="$PACKAGE_ROOT/$label"
  rm -rf "$package_dir"
  mkdir -p "$package_dir/bin"
  cp "$build_binary" "$package_dir/bin/talon"
  chmod 0755 "$package_dir/bin/talon"
  write_package_json "$package_dir" "$package_name" "$os" "$cpu"

  size="$(wc -c < "$package_dir/bin/talon")"
  if (( size > MAX_BYTES )); then
    printf '%s is %s bytes, over limit %s\n' "$package_dir/bin/talon" "$size" "$MAX_BYTES" >&2
    exit 1
  fi
  printf 'packaged %s (%s bytes)\n' "$package_dir" "$size"

  smoke_version "$label" "$package_dir/bin/talon" "$build_binary" "$os" "$cpu"
done
