#!/bin/sh
# AgentWorth installer. POSIX sh, no bashisms.
set -eu

REPO="unfoundbox-crew/agentworth"
API_URL="https://api.github.com/repos/${REPO}/releases/latest"
INSTALL_DIR="${AGENTWORTH_INSTALL_DIR:-$HOME/.local/bin}"
UA="agentworth-install.sh"

# -----------------------------------------------------------------------------
# The brand line
# -----------------------------------------------------------------------------
# Every step prints the one-line Archie form from packages/ui/brand/archie/archie-tui.txt:
#
#   (*) archie  downloading  ──────────·······  68%  15.9 / 23.3 MB
#
# The lamp is the state (* on, o sweeping, - nothing, ' ' error), the label says what is
# happening, the rest is evidence. Glyph set is docs/DESIGN.md's: ASCII, U+2500-259F, and
# the five extras. No emoji, no colour -- the line has to read over ssh and in a CI log
# with the colour stripped, and violet has no honest ANSI-16 equivalent to fall back to.
#
# The three-line figure is deliberately not drawn here. Archie appears once per screen,
# and the installer's screen is the status lines.

# A TTY gets the box-drawing track; anything else gets ASCII, so a downstream tool never
# has to guess a glyph's advance width.
if [ -t 1 ]; then
  TTY=1
  FILL="$(printf '\342\224\200')"   # U+2500 ─
  TRACK="$(printf '\302\267')"      # U+00B7 ·
else
  TTY=0
  FILL="-"
  TRACK="."
fi

# Layout: " (o) archie  " + an 11-column label + two spaces is 26 columns of fixed
# overhead, the same rule the CLI's ui::Ui uses. Below 56 columns the label gives way
# and the line collapses to the lamp, the track and the percent -- ui/views.rs does the
# same thing under ARCHIE_BLOCK_MIN_COLUMNS. 46 columns is the narrowest layout.
columns() {
  _c="${COLUMNS:-}"
  if [ -z "$_c" ] && command -v tput >/dev/null 2>&1; then
    _c="$(tput cols 2>/dev/null || printf '')"
  fi
  case "$_c" in
    '' | *[!0-9]*) _c=80 ;;
  esac
  [ "$_c" -ge 46 ] || _c=46
  [ "$_c" -le 100 ] || _c=100
  printf '%s' "$_c"
}

# Resolved once: a `tput` fork on every redraw is a fork every 200ms.
COLS="$(columns)"

say() { # lamp label rest
  printf ' (%s) archie  %-11s  %s\n' "$1" "$2" "$3"
}

# The error beat: the torch goes out, one line prints, nothing moves after it.
err() {
  say ' ' error "$*" >&2
  exit 1
}

# Searched here, nothing. Not a failure -- the install carries on.
warn() {
  say '-' skipped "$*" >&2
}

repeat() { # string count
  _s=''
  _n="$2"
  while [ "$_n" -gt 0 ]; do
    _s="${_s}$1"
    _n=$((_n - 1))
  done
  printf '%s' "$_s"
}

mb() { # bytes -> "15.9"
  # Tenths of a mebibyte. 23 MB * 10 stays well inside a 32-bit signed int, which the
  # percent below would not: `bytes * 100` overflows dash's arithmetic at ~21 MB.
  _t=$(($1 * 10 / 1048576))
  printf '%d.%d' $((_t / 10)) $((_t % 10))
}

draw_download() { # done total lamp   (total 0 = server sent no Content-Length)
  _done="$1"
  _total="$2"
  _lamp="$3"
  # Under 100 bytes there is no percent to compute without dividing by zero, and no
  # release asset is that small -- so an absent or nonsense Content-Length lands here.
  if [ "$_total" -lt 100 ]; then
    printf '\r (%s) archie  %-11s  %s MB\033[K' "$_lamp" downloading "$(mb "$_done")"
    return 0
  fi

  _pct=$((_done / (_total / 100)))
  [ "$_pct" -le 100 ] || _pct=100

  if [ "$COLS" -ge 56 ]; then
    _bw=$((COLS - 50))
    [ "$_bw" -ge 6 ] || _bw=6
    [ "$_bw" -le 28 ] || _bw=28
    _fill=$((_pct * _bw / 100))
    printf '\r (%s) archie  %-11s  %s%s  %3d%%  %s / %s MB\033[K' \
      "$_lamp" downloading \
      "$(repeat "$FILL" "$_fill")" "$(repeat "$TRACK" $((_bw - _fill)))" \
      "$_pct" "$(mb "$_done")" "$(mb "$_total")"
  else
    _bw=$((COLS - 28))
    [ "$_bw" -ge 6 ] || _bw=6
    [ "$_bw" -le 20 ] || _bw=20
    _fill=$((_pct * _bw / 100))
    printf '\r (%s) archie  %s%s  %3d%%  %s MB\033[K' \
      "$_lamp" \
      "$(repeat "$FILL" "$_fill")" "$(repeat "$TRACK" $((_bw - _fill)))" \
      "$_pct" "$(mb "$_done")"
  fi
}

# `sleep 0.2` is not in POSIX. Every sleep we actually ship against (GNU coreutils, BSD,
# busybox) takes it, but a shell that does not would spin the poll loop at full tilt --
# so it is probed once and the loop falls back to a whole second.
POLL="0.2"
sleep "$POLL" 2>/dev/null || POLL="1"

content_length() {
  # Read from the header dump curl writes while the download streams (-D). The asset URL
  # redirects to GitHub's object CDN, so several responses land in the file; the last
  # Content-Length wins. A separate HEAD request used to do this and could hang for
  # tens of seconds before anything was drawn, so nothing is asked up front any more.
  if [ -f "$1" ]; then
    tr -d '\r' <"$1" |
      tr '[:upper:]' '[:lower:]' |
      awk '/^content-length:/ { n = $2 } END { if (n ~ /^[0-9]+$/) print n; else print 0 }'
  else
    printf '0'
  fi
}

file_bytes() {
  if [ -f "$1" ]; then
    wc -c <"$1" | tr -d ' '
  else
    printf '0'
  fi
}

# -----------------------------------------------------------------------------
# Preflight
# -----------------------------------------------------------------------------

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
  arm64 | aarch64) cpu="aarch64" ;;
  x86_64 | amd64) cpu="x86_64" ;;
  *) err "unsupported architecture: $arch" ;;
esac

triple="${cpu}-${platform}"

# -----------------------------------------------------------------------------
# Resolve
# -----------------------------------------------------------------------------

release_json="$(curl -fsSL --connect-timeout 15 -A "$UA" "$API_URL")" || err "failed to reach $API_URL"

tag="$(echo "$release_json" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n1)"
[ -n "$tag" ] || err "could not determine latest release tag"

asset="agentworth-${tag}-${triple}.tar.gz"
url="https://github.com/${REPO}/releases/download/${tag}/${asset}"

say '*' resolving "$tag  $triple"

tmpdir="$(mktemp -d)"
dl_pid=""
cleanup() {
  [ -z "$dl_pid" ] || kill "$dl_pid" 2>/dev/null || true
  rm -rf "$tmpdir"
}
trap cleanup EXIT INT TERM

# -----------------------------------------------------------------------------
# Download
# -----------------------------------------------------------------------------
# curl runs in the background writing to the file; this shell polls the file's size
# against the Content-Length it asked for up front and draws the bar itself. curl's own
# --progress-bar draws a `#` ruler, which is outside the glyph set, and --progress-meter
# draws a table -- neither can be made to look like anything else, so the size is read
# from the file instead. `wait` is what decides whether the download worked; the poll
# loop only draws.

hdr="$tmpdir/headers"

if [ "$TTY" -eq 0 ]; then
  # No cursor to move, so nothing is redrawn: one line saying what is coming, then
  # silence until it lands. A loop that scrolls is a loop that lies.
  say '*' downloading "$asset"
  curl -fsSL --connect-timeout 15 -A "$UA" -D "$hdr" -o "$tmpdir/$asset" "$url" || err "failed to download $url"
  total="$(content_length "$hdr")"
  if [ "$total" -gt 0 ]; then say '*' downloaded "$(mb "$total") MB"; fi
else
  # `</dev/null` is belt and braces: `curl ... | sh` feeds this script in on stdin, and a
  # background job that inherited it could eat the lines the shell has not read yet. POSIX
  # already redirects a background job's stdin from /dev/null; this makes it not depend on
  # the shell getting that right.
  curl -fsSL --connect-timeout 15 -A "$UA" -D "$hdr" -o "$tmpdir/$asset" "$url" </dev/null &
  dl_pid=$!
  frame=0
  total=0
  draw_download 0 "$total" o
  while kill -0 "$dl_pid" 2>/dev/null; do
    sleep "$POLL"
    # The total is unknown until the CDN answers; re-read the header dump until it is.
    [ "$total" -gt 0 ] || total="$(content_length "$hdr")"
    # The dig loop's rhythm: the lamp holds, then sweeps, ~350ms a frame.
    if [ $((frame % 2)) -eq 0 ]; then lamp='*'; else lamp='o'; fi
    draw_download "$(file_bytes "$tmpdir/$asset")" "$total" "$lamp"
    frame=$((frame + 1))
  done
  dl_rc=0
  wait "$dl_pid" || dl_rc=$?
  dl_pid=""
  if [ "$dl_rc" -ne 0 ]; then
    printf '\r\033[K'
    err "failed to download $url"
  fi
  got="$(file_bytes "$tmpdir/$asset")"
  [ "$total" -gt 0 ] || total="$(content_length "$hdr")"
  [ "$total" -gt 0 ] || total="$got"
  draw_download "$got" "$total" '*'
  printf '\n'
fi

# -----------------------------------------------------------------------------
# Verify
# -----------------------------------------------------------------------------

checksum_url="${url}.sha256"
if curl -fsSL --connect-timeout 15 -A "$UA" -o "$tmpdir/${asset}.sha256" "$checksum_url" 2>/dev/null; then
  (
    cd "$tmpdir" &&
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
      say '*' verifying "sha256 matches"
    else
      warn "checksum: no sha256sum or shasum on this machine"
    fi
  )
else
  warn "checksum: none published for $tag"
fi

# -----------------------------------------------------------------------------
# Extract and install
# -----------------------------------------------------------------------------

tar -xzf "$tmpdir/$asset" -C "$tmpdir" || err "failed to extract $asset"
say '*' extracting "$asset"

mkdir -p "$INSTALL_DIR"

# `archie` is the short name; `agwt` still installs so an existing shell history, alias or
# script keeps working. Both are the same binary.
installed=""
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
    if [ -z "$installed" ]; then installed="$bin"; else installed="$installed, $bin"; fi
  else
    warn "$bin is not in the archive"
  fi
done

[ -n "$installed" ] || err "nothing was installed from $asset"
# `~` rather than the expanded home: the line has to survive an 80-column terminal.
pretty_dir="$(printf '%s' "$INSTALL_DIR" | sed "s|^${HOME}|~|")"
say '*' installed "$installed in $pretty_dir"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    printf '\n'
    printf '  %s is not on your PATH yet:\n' "$INSTALL_DIR"
    printf '    export PATH="%s:$PATH"\n' "$INSTALL_DIR"
    ;;
esac

printf '\n'
printf '  Next  archie --version   confirm the install\n'
