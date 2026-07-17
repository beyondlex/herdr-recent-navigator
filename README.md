# Herdr Recent Navigator

A recent workspaces/tabs/panes switcher for Herdr. Opens an overlay listing
recently focused workspaces, tabs, panes, and AI agents — fuzzy-searchable and
navigable by keyboard.

## Features

- **Four category tabs**: Workspaces, Tabs, Agents, Panes — switch with `Tab`
- **MRU ordering**: most recently focused items float to the top
- **Fuzzy search**: type to filter any category
- **Active-only filter** (`Ctrl+E`): narrow to panes whose agent is blocked or done
- **Live agent status**: Working agents show a braille spinner; status updates
  in real time without reopening
- **Herdr-native colors**: TokyoNight palette, consistent with the Herdr UI
- **Automatic tracking**: hooks into `workspace.focused`, `pane.focused`,
  `tab.focused` events to build MRU history

## Install

### Quick install (curl | bash)

```bash
curl -fsSL https://raw.githubusercontent.com/beyondlex/herdr-recent-navigator/main/install.sh | bash
```

This downloads the prebuilt binary for your platform, places it in
`~/.local/bin/`, and links it into Herdr.

### Manual install

Download the binary for your platform from the
[releases page](https://github.com/beyondlex/herdr-recent-navigator/releases):

```bash
# Replace with your target platform
curl -fsSL https://github.com/beyondlex/herdr-recent-navigator/releases/latest/download/herdr-recent-navigator-aarch64-apple-darwin \
  -o ~/.local/bin/herdr-recent-navigator
chmod +x ~/.local/bin/herdr-recent-navigator
herdr plugin link /path/to/herdr-recent-navigator
```

### Build from source

```bash
git clone https://github.com/beyondlex/herdr-recent-navigator
cd herdr-recent-navigator
cargo build --release
herdr plugin link "$PWD"
```

Verify the plugin is registered:

```bash
herdr plugin action list --plugin recent-navigator
```

## Bind a shortcut

Add to your Herdr config:

```toml
[[keys.command]]
key = "prefix+u"
type = "plugin_action"
command = "beyondlex.herdr-recent-navigator.focus-workspaces"
description = "Open Navigator: Workspace"


# Optional: Focus Tab/Agent when open navigator
[[keys.command]]
key = "cmd+i"
type = "plugin_action"
command = "beyondlex.herdr-recent-navigator.focus-tabs"
description = "Open Navigator: Tab"

[[keys.command]]
key = "cmd+e"
type = "plugin_action"
command = "beyondlex.herdr-recent-navigator.focus-panes"
description = "Open Navigator: Agent"
```

Reload:

```bash
herdr server reload-config
```

Press the shortcut to open the navigator overlay.

## Usage

| Key | Action |
|---|---|
| `↑` / `↓` | Navigate list |
| `Tab` / `Shift+Tab` | Cycle category tabs |
| `Enter` | Focus selected item |
| `Ctrl+E` | Toggle active-only filter |
| `Esc` | Clear search / close |
| `Ctrl+C` | Close without focusing |
| Type any text | Fuzzy-search the list |

### Category tabs

- **Workspaces**: MRU workspaces with dot indicators for agent status
- **Tabs**: MRU tabs within those workspaces
- **Agents**: AI agents sorted by last activity
- **Panes**: Individual terminal panes

## How it works

The navigator is a Herdr plugin that opens as an overlay pane. It maintains a
MRU database in `~/.local/share/herdr/recent-navigator.json` by subscribing to
`workspace.focused`, `pane.focused`, and `tab.focused` events. When opened, it
fetches the current pane/workspace tree via the Herdr CLI, filters by the
selected category, and applies MRU ordering.

While open, it re-fetches pane status every ~2 seconds so agent state changes
(working → done, idle → blocked) reflect immediately without reopening.

### Track mode (event hooks)

The plugin registers three event hooks that fire on focus changes. These write
timestamps to the MRU database. No state is kept in memory between events —
each hook invocation is a one-shot CLI command.

```
herdr-recent-navigator track
```

This is invoked automatically by Herdr when the manifest's `[[events]]` sections
are triggered.
