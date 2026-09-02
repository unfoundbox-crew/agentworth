#!/bin/sh
# AgentWorth installer. POSIX sh, no bashisms.
set -eu

REPO="unfoundbox-crew/agentworth"
API_URL="https://api.github.com/repos/${REPO}/releases/latest"
INSTALL_DIR="${AGENTWORTH_INSTALL_DIR:-$HOME/.local/bin}"

err() {
  echo "error: $*" >&2
  exit 1
}

if [ "$(id -u)" -eq 0 ] && [ "${AGENTWORTH_ALLOW_ROOT:-0}" != "1" ]; then
  err "refusing to run as root. Set AGENTWORTH_ALLOW_ROOT=1 to override."
fi

command -v curl >/dev/null 2>&1 || err "curl is required"
command -v tar >/dev/null 2>&1 || err "tar is required"

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Darwin) platform="apple-darwin" ;;
  Linux) platform="unknown-linux-gnu" ;;
  *) err "unsupported OS: $os" ;;
esac

case "$arch" in
  arm64|aarch64) cpu="aarch64" ;;
  x86_64|amd64) cpu="x86_64" ;;
  *) err "unsupported architecture: $arch" ;;
esac

triple="${cpu}-${platform}"

echo "Resolving latest release..."
release_json="$(curl -fsSL "$API_URL")" || err "failed to reach $API_URL"

tag="$(echo "$release_json" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n1)"
[ -n "$tag" ] || err "could not determine latest release tag"

asset="agentworth-${tag}-${triple}.tar.gz"
url="https://github.com/${REPO}/releases/download/${tag}/${asset}"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT INT TERM

echo "Downloading ${asset} (${tag})..."
curl -fsSL -o "$tmpdir/$asset" "$url" || err "failed to download $url"

checksum_url="${url}.sha256"
if curl -fsSL -o "$tmpdir/${asset}.sha256" "$checksum_url" 2>/dev/null; then
  echo "Verifying checksum..."
  ( cd "$tmpdir" && \
    if command -v sha256sum >/dev/null 2>&1; then
      expected="$(sed -n 's/^\([0-9a-fA-F]*\).*/\1/p' "${asset}.sha256" | head -n1)"
      actual="$(sha256sum "$asset" | awk '{print $1}')"
    elif command -v shasum >/dev/null 2>&1; then
      expected="$(sed -n 's/^\([0-9a-fA-F]*\).*/\1/p' "${asset}.sha256" | head -n1)"
      actual="$(shasum -a 256 "$asset" | awk '{print $1}')"
    else
      expected=""
      actual=""
    fi
    if [ -n "$expected" ] && [ -n "$actual" ]; then
      [ "$expected" = "$actual" ] || err "checksum mismatch for $asset"
    fi
  )
else
  echo "warning: no checksum asset found, skipping verification" >&2
fi

echo "Extracting..."
tar -xzf "$tmpdir/$asset" -C "$tmpdir"

mkdir -p "$INSTALL_DIR"

# `archie` is the short name; `agwt` still installs so an existing shell history, alias or
# script keeps working. Both are the same binary.
for bin in agentworth archie agwt; do
  found=""
  if [ -f "$tmpdir/$bin" ]; then
    found="$tmpdir/$bin"
  else
    found="$(find "$tmpdir" -type f -name "$bin" | head -n1)"
  fi
  if [ -n "$found" ]; then
    cp "$found" "$INSTALL_DIR/$bin"
    chmod +x "$INSTALL_DIR/$bin"
    echo "Installed $bin to $INSTALL_DIR/$bin"
  else
    echo "warning: $bin not found in archive" >&2
  fi
done

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    echo ""
    echo "Add $INSTALL_DIR to your PATH:"
    echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac

echo ""
echo "Done. Run 'archie --version' to verify."
