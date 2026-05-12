#!/usr/bin/env sh
set -eu

usage() {
  cat <<'EOF'
Install the Rust Computer Use sidecar into the bundled computer-use plugin.

Usage:
  scripts/install-computer-use-sidecar.sh [options]

Options:
  --status              Print the current install/build paths and exit.
  --profile debug|release
                        Build/copy profile. Defaults to release.
  --debug               Alias for --profile debug.
  --release             Alias for --profile release.
  --no-build            Copy an existing binary instead of running cargo build.
  --binary PATH         Copy this binary instead of the profile target output.
  --plugin-dir PATH     Override plugin directory. Defaults to the bundled plugin.
  -h, --help            Show this help.

Environment:
  CARGO_TARGET_DIR      Respected when locating cargo output.

Default runtime remains Python. The Rust sidecar is an internal experimental
feature and is not exposed in user settings. Opt into this installed Rust sidecar
for development only with both:
  OMIGA_COMPUTER_USE_EXPERIMENTAL_RUST=1
  OMIGA_COMPUTER_USE_SIDECAR=rust

Choose Rust backend mode with:
  OMIGA_COMPUTER_USE_BACKEND=mock|real|auto
EOF
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
manifest_path="$repo_root/src-tauri/Cargo.toml"
plugin_dir="$repo_root/src-tauri/bundled_plugins/plugins/computer-use"
profile="release"
build=1
status=0
binary_override=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --status)
      status=1
      ;;
    --profile)
      shift
      if [ "$#" -eq 0 ]; then
        echo "--profile requires debug or release" >&2
        exit 2
      fi
      profile="$1"
      ;;
    --debug)
      profile="debug"
      ;;
    --release)
      profile="release"
      ;;
    --no-build)
      build=0
      ;;
    --binary)
      shift
      if [ "$#" -eq 0 ]; then
        echo "--binary requires a path" >&2
        exit 2
      fi
      binary_override="$1"
      ;;
    --plugin-dir)
      shift
      if [ "$#" -eq 0 ]; then
        echo "--plugin-dir requires a path" >&2
        exit 2
      fi
      plugin_dir="$1"
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

case "$profile" in
  debug|release)
    ;;
  *)
    echo "--profile must be debug or release" >&2
    exit 2
    ;;
esac

if [ -n "${CARGO_TARGET_DIR:-}" ]; then
  target_dir="$CARGO_TARGET_DIR"
else
  target_dir="$repo_root/src-tauri/target"
fi

if [ -n "$binary_override" ]; then
  source_binary="$binary_override"
else
  source_binary="$target_dir/$profile/computer-use-sidecar"
fi

dest_dir="$plugin_dir/bin"
dest_binary="$dest_dir/computer-use-sidecar"

sha256_file() {
  if [ ! -f "$1" ]; then
    echo ""
    return
  fi
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
    return
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
    return
  fi
  echo "unavailable"
}

print_status() {
  cat <<EOF
repo_root=$repo_root
manifest_path=$manifest_path
profile=$profile
cargo_target_dir=$target_dir
source_binary=$source_binary
source_exists=$([ -f "$source_binary" ] && echo true || echo false)
source_sha256=$(sha256_file "$source_binary")
plugin_dir=$plugin_dir
dest_binary=$dest_binary
dest_exists=$([ -f "$dest_binary" ] && echo true || echo false)
dest_executable=$([ -x "$dest_binary" ] && echo true || echo false)
dest_sha256=$(sha256_file "$dest_binary")
default_runtime=python
rust_feature=OMIGA_COMPUTER_USE_EXPERIMENTAL_RUST=1
rust_opt_in=OMIGA_COMPUTER_USE_SIDECAR=rust
rust_backend_modes=OMIGA_COMPUTER_USE_BACKEND=mock|real|auto
EOF
}

if [ "$status" -eq 1 ]; then
  print_status
  exit 0
fi

if [ "$build" -eq 1 ]; then
  if [ "$profile" = "release" ]; then
    cargo build --manifest-path "$manifest_path" --bin computer-use-sidecar --release
  else
    cargo build --manifest-path "$manifest_path" --bin computer-use-sidecar
  fi
fi

if [ ! -f "$source_binary" ]; then
  echo "Rust sidecar binary not found: $source_binary" >&2
  echo "Run with --profile debug/release matching your build, or pass --binary PATH." >&2
  exit 1
fi

mkdir -p "$dest_dir"
tmp_binary="$dest_binary.tmp.$$"
cp "$source_binary" "$tmp_binary"
chmod 755 "$tmp_binary"
mv "$tmp_binary" "$dest_binary"

echo "Installed Rust Computer Use sidecar:"
echo "  $dest_binary"
echo "  sha256=$(sha256_file "$dest_binary")"
echo
echo "Runtime remains Python. Rust sidecar is internal/developer-only:"
echo "  OMIGA_COMPUTER_USE_EXPERIMENTAL_RUST=1"
echo "  OMIGA_COMPUTER_USE_SIDECAR=rust"
echo "  OMIGA_COMPUTER_USE_BACKEND=mock|real|auto"
