#!/usr/bin/env sh
# Chameleon one-command installer.
# Usage: curl -sSL https://raw.githubusercontent.com/Haroon966/Chameleon/main/install.sh | sh
# Set CHAMELEON_GITHUB_REPO=Haroon966/Chameleon if different from default.
set -e

GITHUB_REPO="${CHAMELEON_GITHUB_REPO:-Haroon966/Chameleon}"
BINARY_NAME="chameleon"
FORCE=""
if [ "${1:-}" = "-f" ]; then
  FORCE="1"
fi

# Required commands
for cmd in curl tar; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "Error: $cmd is required but not installed." >&2
    exit 1
  fi
done

# Detect OS and arch; map to GitHub release asset pattern
OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS" in
  Linux)
    case "$ARCH" in
      x86_64|amd64)
        # Prefer gnu over musl for broader compatibility
        ASSET_PATTERN="x86_64-unknown-linux-gnu"
        ;;
      aarch64|arm64)
        ASSET_PATTERN="aarch64-unknown-linux-gnu"
        ;;
      *)
        echo "Error: Unsupported architecture: $ARCH" >&2
        exit 1
        ;;
    esac
    ;;
  Darwin)
    case "$ARCH" in
      x86_64)
        ASSET_PATTERN="x86_64-apple-darwin"
        ;;
      arm64|aarch64)
        ASSET_PATTERN="aarch64-apple-darwin"
        ;;
      *)
        echo "Error: Unsupported architecture: $ARCH" >&2
        exit 1
        ;;
    esac
    ;;
  *)
    echo "Error: Unsupported OS: $OS" >&2
    exit 1
    ;;
esac

# If Linux x86_64 and gnu not found, fall back to musl
try_patterns="$ASSET_PATTERN"
if [ "$OS" = "Linux" ] && [ "$ASSET_PATTERN" = "x86_64-unknown-linux-gnu" ]; then
  try_patterns="x86_64-unknown-linux-gnu x86_64-unknown-linux-musl"
fi

echo "Detected: $OS / $ARCH"
echo "Fetching latest release from GitHub..."

release_json=$(curl -sSL -L "https://api.github.com/repos/${GITHUB_REPO}/releases/latest") || true
if [ -z "$release_json" ]; then
  echo "Error: Could not fetch release info. Check network and repo ${GITHUB_REPO}." >&2
  exit 1
fi
if echo "$release_json" | grep -q '"message": "Not Found"'; then
  echo "Error: No releases found for ${GITHUB_REPO}. Check repo name and that a release exists." >&2
  exit 1
fi

# Find first matching asset URL
asset_url=""
for pat in $try_patterns; do
  asset_url=$(echo "$release_json" | grep "browser_download_url" | grep "$pat" | head -1 | sed -n 's/.*"browser_download_url": *"\([^"]*\)".*/\1/p')
  [ -n "$asset_url" ] && break
done

if [ -z "$asset_url" ]; then
  echo "Error: No release asset found for $OS / $ARCH (tried: $try_patterns)." >&2
  exit 1
fi

version=$(echo "$release_json" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)
echo "Installing Chameleon $version..."

tmpdir=""
tmpdir=$(mktemp -d 2>/dev/null || mktemp -d -t chameleon)
trap 'rm -rf "$tmpdir"' EXIT

echo "Downloading..."
curl -sSL -L -o "${tmpdir}/release.tar.gz" "$asset_url"
tar -xzf "${tmpdir}/release.tar.gz" -C "$tmpdir"
# Tarball contains a directory chameleon-<version>-<target>/ with binary inside
binary_src=$(find "$tmpdir" -name "$BINARY_NAME" -type f | head -1)
if [ -z "$binary_src" ] || [ ! -f "$binary_src" ]; then
  echo "Error: Binary not found in archive." >&2
  exit 1
fi

# Choose install directory
install_dir=""
if [ -n "$HOME" ] && [ -d "${HOME}/bin" ] && [ -w "${HOME}/bin" ]; then
  if echo ":$PATH:" | grep -q ":${HOME}/bin:"; then
    install_dir="${HOME}/bin"
  fi
fi
if [ -z "$install_dir" ]; then
  if [ -w "/usr/local/bin" ] 2>/dev/null; then
    install_dir="/usr/local/bin"
  else
    install_dir="/usr/local/bin"
    echo "Installing to $install_dir (may prompt for sudo)."
  fi
fi

target_binary="${install_dir}/${BINARY_NAME}"
if [ -f "$target_binary" ] && [ -z "$FORCE" ]; then
  echo "Already installed: $target_binary"
  printf "Overwrite? [y/N] "
  reply="N"
  read -r reply </dev/tty 2>/dev/null || true
  case "$reply" in
    [yY]|[yY][eE][sS]) ;;
    *) echo "Skipped. Run with -f to force overwrite."; exit 0 ;;
  esac
fi

if [ -w "$install_dir" ]; then
  cp "$binary_src" "$target_binary"
  chmod +x "$target_binary"
else
  sudo cp "$binary_src" "$target_binary"
  sudo chmod +x "$target_binary"
fi

echo "Installed: $target_binary"

# Primary vs Secondary
echo ""
echo "Use Chameleon as:"
echo "  (1) Primary (default terminal) — add desktop entry; optionally set as system default"
echo "  (2) Secondary — only when you run 'chameleon'"
printf "Choice [1/2] (default 2): "
choice="2"
read -r choice </dev/tty 2>/dev/null || true
choice="${choice:-2}"

case "$choice" in
  1)
    if [ "$OS" = "Linux" ]; then
      desktop_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
      mkdir -p "$desktop_dir"
      desktop_file="${desktop_dir}/chameleon.desktop"
      {
        echo '[Desktop Entry]'
        echo 'Name=Chameleon'
        echo 'Comment=Minimal AI-powered terminal emulator'
        echo "Exec=\"$target_binary\""
        echo 'Icon=utilities-terminal'
        echo 'Terminal=false'
        echo 'Type=Application'
        echo 'Categories=System;TerminalEmulator;'
        echo 'StartupNotify=true'
      } > "$desktop_file"
      echo "Desktop entry written: $desktop_file"
      echo "You can set Chameleon as default terminal in your system Settings (Preferred Applications)."
      if command -v update-alternatives >/dev/null 2>&1; then
        printf "Register as system default terminal (x-terminal-emulator, requires sudo)? [y/N] "
        use_alt="N"
        read -r use_alt </dev/tty 2>/dev/null || true
        case "$use_alt" in
          [yY]|[yY][eE][sS])
            sudo update-alternatives --install /usr/bin/x-terminal-emulator x-terminal-emulator "$target_binary" 50
            echo "To switch default: sudo update-alternatives --config x-terminal-emulator"
            ;;
          *) ;;
        esac
      fi
    else
      echo "Primary on macOS: Chameleon is in your PATH. Run 'chameleon' or add it to Dock (right-click the binary in Finder)."
    fi
    ;;
  2|*)
    echo "Run \`chameleon\` from any terminal."
    ;;
esac

echo ""
echo "Done. Chameleon is ready."
