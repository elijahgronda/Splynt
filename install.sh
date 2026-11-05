#!/bin/sh
# Installs or updates splynt, the Splynt terminal app.
#
#   curl -fsSL https://raw.githubusercontent.com/elijahgronda/Splynt/main/install.sh | sh
#
# It puts the binary in ~/.local/bin. Set SPLYNT_INSTALL_DIR to use another
# folder. Mac and Linux (x64) only; on Windows, grab the zip from the release.

set -eu

repo="elijahgronda/Splynt"
dir="${SPLYNT_INSTALL_DIR:-$HOME/.local/bin}"

os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Darwin) asset="splynt-mac.tar.gz" ;;
  Linux)
    case "$arch" in
      x86_64 | amd64) asset="splynt-linux-x64.tar.gz" ;;
      *) asset="" ;;
    esac
    ;;
  *) asset="" ;;
esac

if [ -z "$asset" ]; then
  echo "There's no ready-made splynt for $os $arch yet."
  echo "If you have Rust, this builds it instead:"
  echo "  cargo install --git https://github.com/$repo splynt"
  exit 1
fi

# The newest CLI release. Desktop releases are tagged v*, the CLI cli-v*.
tag=$(curl -fsSL "https://api.github.com/repos/$repo/releases?per_page=50" \
  | grep -o '"tag_name": *"cli-v[^"]*"' \
  | head -n 1 \
  | sed 's/.*"\(cli-v[^"]*\)"/\1/')

if [ -z "$tag" ]; then
  echo "Couldn't find a Splynt CLI release on GitHub."
  exit 1
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "Downloading splynt ${tag#cli-v}..."
curl -fsSL "https://github.com/$repo/releases/download/$tag/$asset" -o "$tmp/$asset"
tar -xzf "$tmp/$asset" -C "$tmp"

mkdir -p "$dir"
mv -f "$tmp/splynt" "$dir/splynt"
chmod 755 "$dir/splynt"
if [ "$os" = "Darwin" ]; then
  xattr -d com.apple.quarantine "$dir/splynt" 2>/dev/null || true
fi

echo "Installed splynt ${tag#cli-v} to $dir/splynt"

case ":$PATH:" in
  *":$dir:"*)
    echo "Run it with: splynt"
    ;;
  *)
    case "${SHELL:-}" in
      */zsh) rc="$HOME/.zshrc" ;;
      */bash) rc="$HOME/.bashrc" ;;
      *) rc="" ;;
    esac
    if [ -n "$rc" ]; then
      if ! grep -qs "export PATH=\"$dir:" "$rc"; then
        printf '\n# splynt\nexport PATH="%s:$PATH"\n' "$dir" >> "$rc"
        echo "Added $dir to your PATH in $rc."
      fi
      echo "Open a new terminal window, then run: splynt"
    else
      echo "Add $dir to your PATH, then run: splynt"
    fi
    ;;
esac
