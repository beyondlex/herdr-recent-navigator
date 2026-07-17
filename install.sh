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
INSTALL_DIR="${HOME}/.local/share/herdr-recent-navigator"
mkdir -p "$INSTALL_DIR"

URL="https://github.com/$REPO/releases/download/$VERSION/$BIN-$TARGET"
echo "Downloading $BIN $VERSION ($TARGET)..."
curl -fsSL "$URL" -o "$INSTALL_DIR/$BIN"
chmod +x "$INSTALL_DIR/$BIN"

echo "Installed to $INSTALL_DIR/$BIN"

# Generate plugin manifest
cat > "$INSTALL_DIR/herdr-plugin.toml" <<PLUGIN_EOF
id = "beyondlex.herdr-recent-navigator"
name = "Herdr Recent Navigator"
version = "$VERSION"
description = "Recent workspaces, tabs, panes, and AI agents switcher for Herdr."
min_herdr_version = "0.7.4"
platforms = ["macos", "linux"]

[[actions]]
id = "open"
title = "Open Navigator"
description = "Open the recent items navigator"
contexts = ["global", "workspace"]
command = ["\$HERDR_PLUGIN_DIR/herdr-recent-navigator", "--pane-open"]

[[actions]]
id = "focus-workspaces"
title = "Quick Focus: Workspaces"
description = "Open navigator focused on Workspaces tab"
contexts = ["global", "workspace"]
command = ["\$HERDR_PLUGIN_DIR/herdr-recent-navigator", "--pane-open", "--view", "workspaces"]

[[actions]]
id = "focus-tabs"
title = "Quick Focus: Tabs"
description = "Open navigator focused on Tabs tab"
contexts = ["global", "workspace"]
command = ["\$HERDR_PLUGIN_DIR/herdr-recent-navigator", "--pane-open", "--view", "tabs"]

[[actions]]
id = "focus-agents"
title = "Quick Focus: Agents"
description = "Open navigator focused on Agents tab"
contexts = ["global", "workspace"]
command = ["\$HERDR_PLUGIN_DIR/herdr-recent-navigator", "--pane-open", "--view", "agents"]

[[actions]]
id = "focus-panes"
title = "Quick Focus: Panes"
description = "Open navigator focused on Panes tab"
contexts = ["global", "workspace"]
command = ["\$HERDR_PLUGIN_DIR/herdr-recent-navigator", "--pane-open", "--view", "panes"]

[[events]]
on = "workspace.focused"
command = ["\$HERDR_PLUGIN_DIR/herdr-recent-navigator", "track"]

[[events]]
on = "pane.focused"
command = ["\$HERDR_PLUGIN_DIR/herdr-recent-navigator", "track"]

[[events]]
on = "tab.focused"
command = ["\$HERDR_PLUGIN_DIR/herdr-recent-navigator", "track"]

[[panes]]
id = "navigator"
title = "Recent Navigator"
placement = "popup"
command = ["\$HERDR_PLUGIN_DIR/herdr-recent-navigator"]
PLUGIN_EOF

# Symlink into PATH
mkdir -p "${HOME}/.local/bin"
ln -sf "$INSTALL_DIR/$BIN" "${HOME}/.local/bin/$BIN"

# Link into herdr from persistent plugin directory
if command -v herdr &>/dev/null; then
  echo "Linking plugin into Herdr..."
  herdr plugin link "$INSTALL_DIR"
  echo "Done! Bind a shortcut to recent-navigator.open in your Herdr config."
else
  echo "Herdr not found. Install Herdr first, then run: herdr plugin link $INSTALL_DIR"
fi
