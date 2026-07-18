#!/usr/bin/env bash
set -euo pipefail

REPO="beyondlex/herdr-recent-navigator"
VERSION="${1:-latest}"

# ── Colors ──────────────────────────────────────────
BOLD='\033[1m'
DIM='\033[2m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[0;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

info()  { printf "  ${CYAN}→${NC}  %s\n" "$*"; }
ok()    { printf "  ${GREEN}✔${NC}  %s\n" "$*"; }
warn()  { printf "  ${YELLOW}⚠${NC}  %s\n" "$*"; }
fail()  { printf "  ${RED}✘${NC}  %s\n" "$*"; exit 1; }
header(){ printf "\n  ${BOLD}%s${NC}\n  ${DIM}%s${NC}\n\n" "━━━ $* ━━━" "────────────────────────────────────────"; }

# ── Detect platform ─────────────────────────────────
header "Checking platform"
ARCH=$(uname -m)
OS=$(uname -s | tr '[:upper:]' '[:lower:]')

case "$OS-$ARCH" in
  linux-x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
  darwin-arm64)  TARGET="aarch64-apple-darwin" ;;
  darwin-x86_64) TARGET="x86_64-apple-darwin" ;;
  *)
    fail "Unsupported platform: $OS $ARCH"
    ;;
esac
ok "Detected $OS ($ARCH) → $TARGET"

# ── Resolve version ──────────────────────────────────
header "Resolving version"
if [ "$VERSION" = "latest" ]; then
  VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed 's/.*"tag_name": "\(.*\)",/\1/')
fi
ok "Version $VERSION"

BIN="herdr-recent-navigator"
INSTALL_DIR="${HOME}/.local/share/herdr-recent-navigator"
mkdir -p "$INSTALL_DIR"

# ── Download binary ──────────────────────────────────
header "Downloading binary"
URL="https://github.com/$REPO/releases/download/$VERSION/$BIN-$TARGET"
info "Fetching $BIN $VERSION ($TARGET)..."
curl -fsSL "$URL" -o "$INSTALL_DIR/$BIN"
chmod +x "$INSTALL_DIR/$BIN"
ok "Installed to $INSTALL_DIR/$BIN"

# ── Generate plugin manifest ─────────────────────────
header "Generating plugin manifest"
cat > "$INSTALL_DIR/herdr-plugin.toml" <<PLUGIN_EOF
id = "beyondlex.herdr-recent-navigator"
name = "Herdr Recent Navigator"
version = "$VERSION"
description = "Recent workspaces, tabs, panes, and AI agents switcher for Herdr."
min_herdr_version = "0.7.4"
platforms = ["macos", "linux"]
theme = "dark"

[[actions]]
id = "open"
title = "Open Navigator"
description = "Open the recent items navigator"
contexts = ["global", "workspace"]
command = ["herdr-recent-navigator", "--pane-open"]

[[actions]]
id = "focus-workspaces"
title = "Quick Focus: Workspaces"
description = "Open navigator focused on Workspaces tab"
contexts = ["global", "workspace"]
command = ["herdr-recent-navigator", "--pane-open", "--view", "workspaces"]

[[actions]]
id = "focus-tabs"
title = "Quick Focus: Tabs"
description = "Open navigator focused on Tabs tab"
contexts = ["global", "workspace"]
command = ["herdr-recent-navigator", "--pane-open", "--view", "tabs"]

[[actions]]
id = "focus-agents"
title = "Quick Focus: Agents"
description = "Open navigator focused on Agents tab"
contexts = ["global", "workspace"]
command = ["herdr-recent-navigator", "--pane-open", "--view", "agents"]

[[actions]]
id = "focus-panes"
title = "Quick Focus: Panes"
description = "Open navigator focused on Panes tab"
contexts = ["global", "workspace"]
command = ["herdr-recent-navigator", "--pane-open", "--view", "panes"]

[[events]]
on = "workspace.focused"
command = ["herdr-recent-navigator", "track"]

[[events]]
on = "pane.focused"
command = ["herdr-recent-navigator", "track"]

[[events]]
on = "tab.focused"
command = ["herdr-recent-navigator", "track"]

[[panes]]
id = "navigator"
title = "Recent Navigator"
placement = "popup"
width = "60%"
command = ["herdr-recent-navigator"]
PLUGIN_EOF

# ── Symlink into PATH ────────────────────────────────
header "Setting up PATH"
mkdir -p "${HOME}/.local/bin"
ln -sf "$INSTALL_DIR/$BIN" "${HOME}/.local/bin/$BIN"
ok "Symlinked to ${HOME}/.local/bin/$BIN"

# ── Link into Herdr ──────────────────────────────────
header "Linking Herdr plugin"
if command -v herdr &>/dev/null; then
  info "Linking plugin into Herdr..."
  herdr plugin link "$INSTALL_DIR"
  printf "\n  ${GREEN}${BOLD}✔ Installation complete!${NC}\n"
  printf "  ${DIM}Bind a shortcut to${NC} ${BOLD}recent-navigator.open${NC} ${DIM}in your Herdr config.${NC}\n"
  printf "  ${DIM}Configure theme at${NC} ${BOLD}%s/herdr-plugin.toml${NC}${DIM}.${NC}\n\n" "$INSTALL_DIR"
else
  warn "Herdr not found. Install Herdr first, then run:"
  printf "  ${CYAN}herdr plugin link${NC} ${DIM}%s${NC}\n" "$INSTALL_DIR"
  printf "  ${DIM}Configure theme at${NC} ${BOLD}%s/herdr-plugin.toml${NC}${DIM}.${NC}\n\n" "$INSTALL_DIR"
fi
