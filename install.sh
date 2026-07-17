#!/usr/bin/env bash
set -euo pipefail

REPO="beyondlex/herdr-recent-navigator"
VERSION="${1:-latest}"

# Detect platform
ARCH=$(uname -m)
OS=$(uname -s | tr '[:upper:]' '[:lower:]')

case "$OS-$ARCH" in
  linux-x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
  darwin-arm64)  TARGET="aarch64-apple-darwin" ;;
  darwin-x86_64) TARGET="x86_64-apple-darwin" ;;
  *)
    echo "Unsupported platform: $OS $ARCH"
    exit 1
    ;;
esac

# Resolve version
if [ "$VERSION" = "latest" ]; then
  VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed 's/.*"tag_name": "\(.*\)",/\1/')
fi

BIN="herdr-recent-navigator"
INSTALL_DIR="${HOME}/.local/bin"
mkdir -p "$INSTALL_DIR"

URL="https://github.com/$REPO/releases/download/$VERSION/$BIN-$TARGET"
echo "Downloading $BIN $VERSION ($TARGET)..."
curl -fsSL "$URL" -o "$INSTALL_DIR/$BIN"
chmod +x "$INSTALL_DIR/$BIN"

echo "Installed to $INSTALL_DIR/$BIN"

# Link into herdr
if command -v herdr &>/dev/null; then
  echo "Linking plugin into Herdr..."
  PROJECT_DIR="$(cd "$(dirname "$0")" && pwd)"
  herdr plugin link "$PROJECT_DIR" 2>/dev/null || {
    # If not inside the repo, create a minimal plugin manifest
    TMP_DIR=$(mktemp -d)
    cat > "$TMP_DIR/herdr-plugin.toml" <<'EOF'
id = "recent-navigator"
name = "Herdr Recent Navigator"
version = "0.1.0"
description = "Recent workspaces/tabs/panes switcher for Herdr."
min_herdr_version = "0.7.0"
platforms = ["macos", "linux"]

[[actions]]
id = "open"
title = "Open Navigator"
description = "Open the recent items navigator"
contexts = ["global", "workspace"]
command = ["$HERDR_PLUGIN_DIR/herdr-recent-navigator", "--pane-open"]

[[events]]
on = "workspace.focused"
command = ["$HERDR_PLUGIN_DIR/herdr-recent-navigator", "track"]

[[events]]
on = "pane.focused"
command = ["$HERDR_PLUGIN_DIR/herdr-recent-navigator", "track"]

[[events]]
on = "tab.focused"
command = ["$HERDR_PLUGIN_DIR/herdr-recent-navigator", "track"]

[[panes]]
id = "navigator"
title = "Recent Navigator"
placement = "overlay"
command = ["$HERDR_PLUGIN_DIR/herdr-recent-navigator"]
EOF
    herdr plugin link "$TMP_DIR"
    rm -rf "$TMP_DIR"
  }
  echo "Done! Bind a shortcut to recent-navigator.open in your Herdr config."
else
  echo "Herdr not found. Install Herdr first, then run: herdr plugin link <this-directory>"
fi
