#!/usr/bin/env bash
#
# Install the aqs binary from a GitHub Release.
#
#   ./install.sh                      # latest release into ~/.local/bin
#   ./install.sh v0.3.0               # a specific tag
#   PREFIX=/usr/local/bin ./install.sh
#
# Piped straight from the repository:
#
#   curl -fsSL -H "Authorization: Bearer $(gh auth token)" \
#     https://raw.githubusercontent.com/Artifer/artifer-quality-score/master/install.sh \
#     | GH_TOKEN="$(gh auth token)" bash
#
# The repository is private, so every download needs credentials — GitHub does not
# serve release assets of a private repository anonymously. Two paths are supported:
#
#   token   GH_TOKEN / GITHUB_TOKEN, using nothing but curl and tar. This is the
#           path for CI containers and anywhere `gh` is not installed.
#   gh      Falls back to `gh release download` when no token is set, which every
#           developer here already has authenticated.
#
set -euo pipefail

REPO="${AQS_REPO:-Artifer/artifer-quality-score}"
TAG="${1:-}"
PREFIX="${PREFIX:-$HOME/.local/bin}"
API="https://api.github.com/repos/${REPO}"

die() { echo "error: $*" >&2; exit 1; }

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
if [ -z "$token" ] && command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  token="$(gh auth token 2>/dev/null || true)"
fi

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
tarball="$workdir/aqs.tar.gz"

if [ -n "$token" ]; then
  command -v curl >/dev/null 2>&1 || die "curl is required"

  release_url="$API/releases/latest"
  [ -n "$TAG" ] && release_url="$API/releases/tags/$TAG"

  release_json="$workdir/release.json"
  curl -fsSL -H "Authorization: Bearer $token" \
       -H "Accept: application/vnd.github+json" \
       "$release_url" -o "$release_json" \
    || die "cannot read the release. Is the token valid and does it have repo read access?"

  # Find the asset id without requiring jq. The API pretty-prints, so id and name sit
  # on separate lines; in a GitHub asset object "id" always precedes "name", so the
  # last id seen before the matching name is the one we want. `"node_id"` cannot be
  # confused with `"id"` because the pattern demands a quote immediately before it.
  asset_id="$(awk -v want="${asset}.tar.gz" '
    /"id"[[:space:]]*:[[:space:]]*[0-9]+/ {
      if (match($0, /[0-9]+/)) last = substr($0, RSTART, RLENGTH)
    }
    /"name"[[:space:]]*:/ {
      if (index($0, want) > 0) { print last; exit }
    }
  ' "$release_json")"
  [ -n "$asset_id" ] || die "no asset matching *${asset}.tar.gz in that release"

  echo "Downloading aqs (${asset})..."
  curl -fsSL -H "Authorization: Bearer $token" \
       -H "Accept: application/octet-stream" \
       "$API/releases/assets/${asset_id}" -o "$tarball" \
    || die "download failed"
else
  command -v gh >/dev/null 2>&1 \
    || die "set GH_TOKEN (or GITHUB_TOKEN), or install gh — the repository is private"
  echo "Downloading aqs (${asset}) via gh..."
  if [ -n "$TAG" ]; then
    gh release download "$TAG" --repo "$REPO" --pattern "*${asset}.tar.gz" --dir "$workdir"
  else
    gh release download --repo "$REPO" --pattern "*${asset}.tar.gz" --dir "$workdir"
  fi
  tarball="$(find "$workdir" -name '*.tar.gz' -print -quit)"
  [ -n "$tarball" ] || die "no asset matched *${asset}.tar.gz"
fi

tar -xzf "$tarball" -C "$workdir"
[ -f "$workdir/aqs" ] || die "the archive did not contain an aqs binary"
mkdir -p "$PREFIX"
install -m 755 "$workdir/aqs" "$PREFIX/aqs"

echo "Installed $("$PREFIX/aqs" --version) to $PREFIX/aqs"
case ":$PATH:" in
  *":$PREFIX:"*) ;;
  *) echo "note: $PREFIX is not on your PATH. Add it to your shell profile:"
     echo "      export PATH=\"$PREFIX:\$PATH\"" ;;
esac
