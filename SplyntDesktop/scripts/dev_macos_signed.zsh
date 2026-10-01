#!/bin/zsh

set -euo pipefail

if [[ "$(/usr/bin/uname -s)" != "Darwin" ]]; then
  print -u2 "The signed development runner is only available on macOS."
  exit 1
fi

script_dir="${0:A:h}"
project_dir="${script_dir:h}"
tauri_dir="$project_dir/src-tauri"

identity="${APPLE_SIGNING_IDENTITY:-}"
if [[ -z "$identity" ]]; then
  identities=$(/usr/bin/security find-identity -v -p codesigning \
    | /usr/bin/sed -nE 's/^[[:space:]]*[0-9]+\) [A-F0-9]+ "((Developer ID Application|Apple Development):[^"]+)"$/\1/p' \
  )
  identity=$(print -r -- "$identities" | /usr/bin/sed -n '/^Developer ID Application:/p' | /usr/bin/head -n 1)
  if [[ -z "$identity" ]]; then
    identity=$(print -r -- "$identities" | /usr/bin/sed -n '/^Apple Development:/p' | /usr/bin/head -n 1)
  fi
fi

if [[ -z "$identity" || "$identity" == "-" ]]; then
  print -u2 "No stable macOS code-signing identity was found. Install an Apple Development certificate with Xcode."
  exit 1
fi

if (( $# == 0 )) || [[ "$1" != "run" ]]; then
  print -u2 "Expected Tauri to invoke the signed runner as: cargo run [options] -- [app arguments]"
  exit 2
fi
shift

typeset -a build_args app_args
build_args=()
app_args=()
profile="debug"
target_triple=""
binary_name="splynt-desktop"

while (( $# > 0 )); do
  case "$1" in
    --)
      shift
      app_args=("$@")
      break
      ;;
    --release)
      profile="release"
      build_args+=("$1")
      ;;
    --profile)
      if (( $# < 2 )); then
        print -u2 "Missing value after --profile."
        exit 2
      fi
      build_args+=("$1" "$2")
      profile="$2"
      [[ "$profile" == "dev" ]] && profile="debug"
      shift
      ;;
    --profile=*)
      build_args+=("$1")
      profile="${1#--profile=}"
      [[ "$profile" == "dev" ]] && profile="debug"
      ;;
    --target)
      if (( $# < 2 )); then
        print -u2 "Missing value after --target."
        exit 2
      fi
      build_args+=("$1" "$2")
      target_triple="$2"
      shift
      ;;
    --target=*)
      build_args+=("$1")
      target_triple="${1#--target=}"
      ;;
    --bin)
      if (( $# < 2 )); then
        print -u2 "Missing value after --bin."
        exit 2
      fi
      build_args+=("$1" "$2")
      binary_name="$2"
      shift
      ;;
    --bin=*)
      build_args+=("$1")
      binary_name="${1#--bin=}"
      ;;
    *)
      build_args+=("$1")
      ;;
  esac
  shift
done

cd "$tauri_dir"

print "Building Splynt for signed macOS development..."
cargo build "${build_args[@]}"

target_dir="${CARGO_TARGET_DIR:-target}"
if [[ "$target_dir" != /* ]]; then
  target_dir="$tauri_dir/$target_dir"
fi
if [[ -n "$target_triple" ]]; then
  target_dir="$target_dir/$target_triple"
fi

binary_path="$target_dir/$profile/$binary_name"
if [[ ! -f "$binary_path" ]]; then
  print -u2 "Cargo completed, but the expected Splynt executable was not found at $binary_path"
  exit 1
fi

print "Signing Splynt with stable identity: $identity"
/usr/bin/codesign \
  --force \
  --sign "$identity" \
  --identifier "com.splice.desktop" \
  --timestamp=none \
  "$binary_path"
/usr/bin/codesign --verify --strict --verbose=2 "$binary_path"

print "Launching signed Splynt development build."
exec "$binary_path" "${app_args[@]}"
