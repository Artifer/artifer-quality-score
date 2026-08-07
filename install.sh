#!/usr/bin/env bash
#
# Install the aqs binary from the latest GitHub Release.
#
# The repository is private, so this uses `gh` for authentication rather than a bare
# curl: every Artifer developer already has `gh` logged in, and the alternative is
# minting and distributing personal access tokens.
#
#   ./install.sh                 # latest release, into ~/.local/bin
#   ./install.sh v0.3.0          # a specific tag
#   PREFIX=/usr/local/bin ./install.sh
#
set -euo pipefail

REPO="Artifer/artifer-quality-score"
TAG="${1:-}"
PREFIX="${PREFIX:-$HOME/.local/bin}"

if ! command -v gh >/dev/null 2>&1; then
  echo "error: gh is required (brew install gh), since the repository is private." >&2
  exit 1
fi
if ! gh auth status >/dev/null 2>&1; then
  echo "error: not logged in. Run: gh auth login" >&2
  exit 1
fi

# Map uname output onto the asset naming used by the release workflow.
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Darwin) asset="macos-universal" ;;
  Linux)
    case "$arch" in
      x86_64)          asset="x86_64-unknown-linux-gnu" ;;
      aarch64|arm64)   asset="aarch64-unknown-linux-gnu" ;;
      *) echo "error: unsupported Linux architecture: $arch" >&2; exit 1 ;;
    esac
    ;;
  *) echo "error: unsupported platform: $os" >&2; exit 1 ;;
esac

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

echo "Downloading aqs (${asset})..."
if [ -n "$TAG" ]; then
  gh release download "$TAG" --repo "$REPO" --pattern "*${asset}.tar.gz" --dir "$workdir"
else
  gh release download --repo "$REPO" --pattern "*${asset}.tar.gz" --dir "$workdir"
fi

tarball="$(find "$workdir" -name '*.tar.gz' -print -quit)"
if [ -z "$tarball" ]; then
  echo "error: no asset matched *${asset}.tar.gz" >&2
  exit 1
fi

tar -xzf "$tarball" -C "$workdir"
mkdir -p "$PREFIX"
install -m 755 "$workdir/aqs" "$PREFIX/aqs"

echo "Installed $("$PREFIX/aqs" --version) to $PREFIX/aqs"
case ":$PATH:" in
  *":$PREFIX:"*) ;;
  *) echo "note: $PREFIX is not on your PATH. Add it to your shell profile:"
     echo "      export PATH=\"$PREFIX:\$PATH\"" ;;
esac
