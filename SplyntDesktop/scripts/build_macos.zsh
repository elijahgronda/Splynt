#!/bin/zsh

set -euo pipefail

cd "${0:A:h}/.."

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

if [[ -z "$identity" ]]; then
  print -u2 "No valid macOS code-signing identity was found. Install one with Xcode before building Splynt."
  exit 1
fi

print "Building Splynt with stable signing identity: $identity"
APPLE_SIGNING_IDENTITY="$identity" npm run tauri build -- --bundles app,dmg
