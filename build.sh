#!/usr/bin/env bash
# Builds the CLI (nbw) and the GUI (nbw-gui). Both end up in ./target/release
# Works on Linux, macOS and Git Bash on Windows.
#
#   ./build.sh              build everything
#   ./build.sh --cli-only   skip the GUI (faster; on Windows the scheduler needs nbw-gui)
#   ./build.sh --run        build, then open the GUI
#   ./build.sh --clean      delete build output first
#   ./build.sh --package    also make the installer and a portable zip (Windows/Git Bash only)
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

CLI_ONLY=0; RUN=0; CLEAN=0; PACKAGE=0
for arg in "$@"; do
  case "$arg" in
    --package)  PACKAGE=1 ;;
    --cli-only) CLI_ONLY=1 ;;
    --run)      RUN=1 ;;
    --clean)    CLEAN=1 ;;
    -h|--help)  sed -n '2,9p' "$0"; exit 0 ;;
    *) echo "Unknown option: $arg (try --help)" >&2; exit 2 ;;
  esac
done

# Binaries get an .exe suffix on Windows.
case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) EXE=".exe" ;; *) EXE="" ;; esac

# Check --package up front so a long build doesn't end in an error.
if [ "$PACKAGE" = 1 ]; then
  [ "$CLI_ONLY" = 0 ] || { echo "--package needs the GUI; drop --cli-only." >&2; exit 2; }
  [ -n "$EXE" ] || { echo "--package is Windows-only for now (NSIS installer). Use Git Bash on Windows or .\\build.ps1 -Package." >&2; exit 2; }
  command -v npx >/dev/null 2>&1 || { echo "npx not found. Install Node.js (only used to run the Tauri bundler)." >&2; exit 1; }
fi

# cargo may not be on PATH in terminals opened before Rust was installed.
if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo$EXE" ]; then
  export PATH="$PATH:$HOME/.cargo/bin"
fi
command -v cargo >/dev/null 2>&1 || { echo "cargo not found. Install Rust from https://rustup.rs" >&2; exit 1; }

step() { printf '\033[36m==> %s\033[0m\n' "$1"; }

if [ "$CLEAN" = 1 ]; then
  step "Clean"
  cargo clean   # the GUI shares the same target dir
fi

step "Build CLI (nbw)"
cargo build --release

if [ "$CLI_ONLY" = 0 ]; then
  if [ "$(uname -s)" = "Linux" ]; then
    echo "Note: the GUI needs webkit2gtk and friends on Linux (see https://tauri.app/start/prerequisites/)."
  fi
  step "Build GUI (nbw-gui)"
  cargo build --release --manifest-path gui/src-tauri/Cargo.toml --target-dir target
fi

if [ "$PACKAGE" = 1 ]; then
  step "Build installer (Tauri bundler)"
  # Runs from gui/src-tauri so Cargo picks up its .cargo/config.toml (shared target dir).
  (cd gui/src-tauri && npx --yes "@tauri-apps/cli@2.12.6" build)

  step "Build portable zip"
  BUNDLE="$PWD/target/release/bundle"
  ZIP="$BUNDLE/no-bloat-wallpaper-windows-x64.zip"
  mkdir -p "$BUNDLE"
  rm -f "$ZIP"
  # Flat zip, same contents as build.ps1. Git Bash rarely ships zip, so fall back to PowerShell.
  if command -v zip >/dev/null 2>&1; then
    zip -j -q "$ZIP" target/release/nbw.exe target/release/nbw-gui.exe LICENSE README.md DISCLAIMER.md
  else
    powershell.exe -NoProfile -Command "Compress-Archive -DestinationPath '$(cygpath -w "$ZIP")' -Path 'target\release\nbw.exe','target\release\nbw-gui.exe','LICENSE','README.md','DISCLAIMER.md'"
  fi

  printf '\n\033[32mPackages in %s:\033[0m\n' "$BUNDLE"
  find "$BUNDLE" -type f \( -name '*.exe' -o -name '*.zip' \) | while IFS= read -r f; do
    printf '  %s  (%s)\n' "${f#"$BUNDLE"/}" "$(du -h "$f" | cut -f1)"
  done
fi

OUT="$PWD/target/release"
printf '\n\033[32mDone. Output in %s:\033[0m\n' "$OUT"
for n in "nbw$EXE" "nbw-gui$EXE"; do
  [ -f "$OUT/$n" ] && ls -lh "$OUT/$n" | awk -v n="$n" '{print "  " n "  (" $5 ")"}'
done

if [ "$RUN" = 1 ] && [ "$CLI_ONLY" = 0 ]; then
  "$OUT/nbw-gui$EXE" &
fi
