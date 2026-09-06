#!/bin/sh
# qoral installer for Linux and macOS (and WSL2).
#
#   curl -fsSL https://raw.githubusercontent.com/shreeyanshujha/qoral/main/install.sh | sh
#   ./install.sh                     # from a clone: prefers a prebuilt release, else builds with cargo
#   ./install.sh --build             # force a local cargo build
#   QORAL_BIN_DIR=/some/bin ./install.sh
#   QORAL_VERSION=v0.3.0 ./install.sh
set -eu

REPO="shreeyanshujha/qoral"
red()    { printf '\033[31m%s\033[0m\n' "$*"; }
green()  { printf '\033[32m%s\033[0m\n' "$*"; }
yellow() { printf '\033[33m%s\033[0m\n' "$*"; }

force_build=0
for a in "$@"; do [ "$a" = "--build" ] && force_build=1; done

os="$(uname -s)"; arch="$(uname -m)"
case "$os" in
  Darwin) platform=macOS; target_os=apple-darwin ;;
  Linux)  if grep -qi microsoft /proc/version 2>/dev/null; then platform=WSL2; else platform=Linux; fi; target_os=unknown-linux-gnu ;;
  MINGW*|MSYS*|CYGWIN*) red "Use WSL2 on Windows for now (native Windows support is in progress)."; exit 1 ;;
  *) red "unsupported OS: $os"; exit 1 ;;
esac
case "$arch" in
  x86_64|amd64) target_arch=x86_64 ;;
  arm64|aarch64) target_arch=aarch64 ;;
  *) red "unsupported architecture: $arch"; exit 1 ;;
esac
target="$target_arch-$target_os"
echo "qoral installer · $platform · $target"

# --- destination ---
if [ -n "${QORAL_BIN_DIR:-}" ]; then dest_dir="$QORAL_BIN_DIR"
elif [ -d "$HOME/.local/bin" ]; then dest_dir="$HOME/.local/bin"
elif [ "$platform" = macOS ] && [ -w /opt/homebrew/bin ]; then dest_dir=/opt/homebrew/bin
elif [ "$platform" = macOS ] && [ -w /usr/local/bin ]; then dest_dir=/usr/local/bin
else dest_dir="$HOME/.local/bin"; fi
mkdir -p "$dest_dir"

here="$(cd "$(dirname "$0")" 2>/dev/null && pwd || true)"
in_clone=0; [ -n "$here" ] && [ -f "$here/Cargo.toml" ] && grep -q '^name = "qoral"' "$here/Cargo.toml" && in_clone=1

installed=0

# --- 1. prebuilt release ---
if [ "$force_build" = 0 ]; then
  if command -v curl >/dev/null 2>&1; then
    ver="${QORAL_VERSION:-}"
    if [ -z "$ver" ]; then
      ver="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1 || true)"
    fi
    if [ -n "$ver" ]; then
      url="https://github.com/$REPO/releases/download/$ver/qoral-$ver-$target.tar.gz"
      tmp="$(mktemp -d)"
      if curl -fsSL "$url" -o "$tmp/qoral.tar.gz" 2>/dev/null; then
        tar -xzf "$tmp/qoral.tar.gz" -C "$tmp"
        bin="$(find "$tmp" -type f -name qoral | head -1)"
        if [ -n "$bin" ]; then
          install -m 755 "$bin" "$dest_dir/qoral"
          green "installed prebuilt qoral $ver -> $dest_dir/qoral"
          installed=1
        fi
      else
        yellow "no prebuilt binary for $target at $ver (or offline); falling back to a local build"
      fi
      rm -rf "$tmp"
    else
      yellow "could not determine the latest release; falling back to a local build"
    fi
  fi
fi

# --- 2. build from source ---
if [ "$installed" = 0 ]; then
  if ! command -v cargo >/dev/null 2>&1; then
    red "cargo (Rust) not found and no prebuilt binary was available."
    echo "  install Rust:  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    [ "$platform" = macOS ] && echo "  or:            brew install rust"
    echo "  then re-run this script."
    exit 1
  fi
  if [ "$in_clone" = 1 ]; then
    echo "building from source in $here (release profile; a few minutes the first time)…"
    (cd "$here" && cargo build --release --quiet)
    install -m 755 "$here/target/release/qoral" "$dest_dir/qoral"
    green "built and installed -> $dest_dir/qoral"
  else
    echo "installing from git with cargo…"
    cargo install --quiet --git "https://github.com/$REPO" --locked qoral --root "$(dirname "$dest_dir")" 2>/dev/null || cargo install --git "https://github.com/$REPO" qoral --root "$(dirname "$dest_dir")"
    green "installed via cargo -> $dest_dir/qoral"
  fi
fi

case ":$PATH:" in
  *":$dest_dir:"*) ;;
  *) yellow "$dest_dir is not on your PATH. Add to your shell rc:"; echo "  export PATH=\"$dest_dir:\$PATH\"" ;;
esac

echo
echo "agent CLIs:"
for h in claude codex agy gemini; do
  if command -v "$h" >/dev/null 2>&1; then green "  $h"; else yellow "  $h (not found)"; fi
done
echo
case "$platform" in
  macOS) echo "Tip: make Option send Meta so Alt chords work (Terminal.app: Keyboard → Use Option as Meta key; iTerm2: Left Option → Esc+)." ;;
  WSL2)  echo "Tip: Windows Terminal uses Alt+arrows for its own panes; use Alt+h / Alt+l in qoral." ;;
esac
echo "Done. Run: qoral doctor   then: qoral"
