#!/bin/sh
# qoral installer for Linux, macOS and WSL2.
# Checks Node >= 22.13 and tmux >= 3.2, then links `qoral` into a bin directory on your PATH.
#   ./install.sh                  -> ~/.local/bin/qoral (Linux/WSL) or /usr/local/bin (macOS if writable) …
#   QORAL_BIN_DIR=/some/bin ./install.sh
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
bin="$here/bin/qoral.js"

red()   { printf '\033[31m%s\033[0m\n' "$*"; }
green() { printf '\033[32m%s\033[0m\n' "$*"; }
yellow(){ printf '\033[33m%s\033[0m\n' "$*"; }

os="$(uname -s)"
case "$os" in
  Darwin) platform=macOS ;;
  Linux)  if grep -qi microsoft /proc/version 2>/dev/null; then platform=WSL2; else platform=Linux; fi ;;
  MINGW*|MSYS*|CYGWIN*) red "Git Bash / MSYS is not supported: tmux is required. Use WSL2 (see windows/install.ps1)."; exit 1 ;;
  *) platform="$os" ;;
esac
echo "qoral installer · $platform"

# --- node ---
if ! command -v node >/dev/null 2>&1; then
  red "node not found."
  case "$platform" in
    macOS) echo "  brew install node" ;;
    *)     echo "  install Node 22.13+ (e.g. https://nodejs.org, mise, nvm, or your package manager)" ;;
  esac
  exit 1
fi
nodev="$(node -p 'process.versions.node')"
if ! node -e 'const [a,b]=process.versions.node.split(".").map(Number); process.exit(a>22||(a===22&&b>=13)?0:1)'; then
  red "node $nodev is too old; need >= 22.13 (built-in node:sqlite)."; exit 1
fi
green "node $nodev"

# --- tmux ---
if ! command -v tmux >/dev/null 2>&1; then
  red "tmux not found."
  case "$platform" in
    macOS) echo "  brew install tmux" ;;
    *)     echo "  sudo apt install tmux   |   sudo pacman -S tmux   |   sudo dnf install tmux" ;;
  esac
  exit 1
fi
tmuxv="$(tmux -V | sed 's/[^0-9.]*\([0-9][0-9.]*\).*/\1/')"
# version compare via node (BSD sort on macOS has no -V)
if ! node -e 'const [a,b]=process.argv[1].split(".").map(Number); process.exit(a>3||(a===3&&(b||0)>=2)?0:1)' "$tmuxv"; then
  red "tmux $tmuxv is too old; need >= 3.2."; exit 1
fi
green "tmux $tmuxv"

if ! infocmp tmux-256color >/dev/null 2>&1; then
  yellow "terminfo has no tmux-256color entry; qoral will fall back to screen-256color (fine)."
  [ "$platform" = macOS ] && echo "  optional fix: brew install ncurses"
fi

# --- link ---
if [ -n "${QORAL_BIN_DIR:-}" ]; then
  dest_dir="$QORAL_BIN_DIR"
elif [ -d "$HOME/.local/bin" ]; then
  dest_dir="$HOME/.local/bin"
elif [ "$platform" = macOS ] && [ -w /usr/local/bin ]; then
  dest_dir=/usr/local/bin
elif [ "$platform" = macOS ] && [ -w /opt/homebrew/bin ]; then
  dest_dir=/opt/homebrew/bin
else
  dest_dir="$HOME/.local/bin"
fi
mkdir -p "$dest_dir"
chmod +x "$bin"
ln -sfn "$bin" "$dest_dir/qoral"
green "linked $dest_dir/qoral -> $bin"

case ":$PATH:" in
  *":$dest_dir:"*) ;;
  *) yellow "$dest_dir is not on your PATH. Add to your shell rc:"; echo "  export PATH=\"$dest_dir:\$PATH\"" ;;
esac

# --- agents ---
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
