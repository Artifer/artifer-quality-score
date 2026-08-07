#!/usr/bin/env bash
#
# Install the aqs binary from a GitHub Release.
#
#   curl -fsSL https://raw.githubusercontent.com/Artifer/artifer-quality-score/master/install.sh | bash
#
#   ./install.sh                       # latest release into ~/.local/bin
#   ./install.sh v0.3.0                # pin a version
#   PREFIX=/usr/local/bin ./install.sh
#
# Needs only curl and tar. The download is checksum-verified against the SHA256SUMS
# published alongside the release — worth doing for anything piped into a shell.
#
# GH_TOKEN / GITHUB_TOKEN are honoured if set, which matters only for API rate limits
# or if you have forked this into a private repository.
#
set -euo pipefail

REPO="${AQS_REPO:-Artifer/artifer-quality-score}"
TAG="${1:-}"
PREFIX="${PREFIX:-$HOME/.local/bin}"

die() { echo "error: $*" >&2; exit 1; }

command -v curl >/dev/null 2>&1 || die "curl is required"
command -v tar  >/dev/null 2>&1 || die "tar is required"

# Map uname onto the asset naming used by .github/workflows/release.yml.
case "$(uname -s)" in
  Darwin) asset="macos-universal" ;;
  Linux)
    case "$(uname -m)" in
      x86_64)        asset="x86_64-unknown-linux-gnu" ;;
      aarch64|arm64) asset="aarch64-unknown-linux-gnu" ;;
      *) die "unsupported Linux architecture: $(uname -m)" ;;
    esac
    ;;
  *) die "unsupported platform: $(uname -s)" ;;
esac

token="${GH_TOKEN:-${GITHUB_TOKEN:-}}"

# A function rather than an array of curl flags: macOS still ships bash 3.2, where
# expanding an empty array under `set -u` is an unbound-variable error.
fetch() { # fetch <url> <output-path>
  if [ -n "$token" ]; then
    curl -fsSL -H "Authorization: Bearer $token" "$1" -o "$2"
  else
    curl -fsSL "$1" -o "$2"
  fi
}

if [ -z "$TAG" ]; then
  latest="$(mktemp)"
  fetch "https://api.github.com/repos/${REPO}/releases/latest" "$latest" \
    || die "could not reach the GitHub API to resolve the latest release"
  TAG="$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$latest" | head -1)"
  rm -f "$latest"
  [ -n "$TAG" ] || die "could not resolve the latest release tag"
fi

name="aqs-${TAG}-${asset}.tar.gz"
base="https://github.com/${REPO}/releases/download/${TAG}"

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

echo "Downloading ${name}..."
fetch "${base}/${name}" "$workdir/$name" \
  || die "no such asset: ${base}/${name}"

# Verify integrity. A missing SHA256SUMS is a warning rather than fatal so that an
# older release without one can still be installed.
if fetch "${base}/SHA256SUMS" "$workdir/SHA256SUMS" 2>/dev/null; then
  expected="$(awk -v f="$name" '$2 == f || $2 == "*"f { print $1 }' "$workdir/SHA256SUMS")"
  if [ -n "$expected" ]; then
    if command -v sha256sum >/dev/null 2>&1; then
      actual="$(sha256sum "$workdir/$name" | cut -d' ' -f1)"
    else
      actual="$(shasum -a 256 "$workdir/$name" | cut -d' ' -f1)"
    fi
    [ "$actual" = "$expected" ] || die "checksum mismatch
  expected $expected
  actual   $actual"
    echo "Checksum verified."
  fi
else
  echo "warning: no SHA256SUMS published for ${TAG}; skipping verification" >&2
fi

tar -xzf "$workdir/$name" -C "$workdir"
[ -f "$workdir/aqs" ] || die "the archive did not contain an aqs binary"
mkdir -p "$PREFIX"
install -m 755 "$workdir/aqs" "$PREFIX/aqs"

echo "Installed $("$PREFIX/aqs" --version) to $PREFIX/aqs"
case ":$PATH:" in
  *":$PREFIX:"*) ;;
  *) echo "note: $PREFIX is not on your PATH. Add it to your shell profile:"
     echo "      export PATH=\"$PREFIX:\$PATH\"" ;;
esac
