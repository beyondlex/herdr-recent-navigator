# Herdr Recent Navigator

A recent workspaces/tabs/panes switcher for [Herdr](https://herdr.dev/). Opens an popup listing
recently focused workspaces, tabs, panes, and AI agents — fuzzy-searchable and
navigable by keyboard.

![Screenshot](https://github.com/beyondlex/images/blob/main/recent-navigator.jpg)

<p align="center">
  <img alt="Herdr 0.7.4+" src="https://img.shields.io/badge/Herdr-0.7.4%2B-6693ff" />
  <img alt="Linux and macOS" src="https://img.shields.io/badge/Platform-Linux%20%7C%20macOS-2eb14f" />
  <img alt="Release" src="https://img.shields.io/github/v/release/beyondlex/herdr-recent-navigator" />
  <a href="LICENSE"><img alt="MIT License" src="https://img.shields.io/badge/License-MIT-cd933e" /></a>
</p>

## Demo

<p align="center">
  <img alt="demo" src="https://github.com/beyondlex/images/blob/main/recent-navigator.gif" width="559px" />
</p>
<p>
  <img alt="cmd" src="https://github.com/beyondlex/images/blob/main/recent_navigator_cmd.png" />
</p>
<p>
  <img alt="cmd" src="https://github.com/beyondlex/images/blob/main/recent-navigator-content.png" />
</p>

## Features

- **Four category tabs**: Workspaces, Tabs, Agents, Panes — switch with `Tab`
- **MRU ordering**: most recently focused items float to the top
- **Fuzzy search**: type to filter any category
- **Customizable quick-jump shortcuts**: Bind separate keys to open each tab
  directly — e.g. `prefix+u` → Workspaces, `cmd+i` → Tabs,
  `cmd+e` → Agents, `cmd+shift+n` → Panes
- **Cross-category filtering**: Open the Agents tab and fuzzy-filter by
  workspace name to find all agents under a specific workspace; similarly
  filter Panes by tab name, or Tabs by workspace — no need to navigate
  through the tree
- **Live agent status**: Working agents show a braille spinner; status updates
  in real time without reopening
- **Herdr-native colors**: TokyoNight palette, consistent with the Herdr UI
- **Automatic tracking**: hooks into `workspace.focused`, `pane.focused`,
  `tab.focused` events to build `MRU` history

## Install

> **Warning:** Requires Herdr **≥ 0.7.4**. Check with `herdr -V`.  
> To upgrade Herdr, see [herdr.dev/docs/install/#update](https://herdr.dev/docs/install/#update).

Choose one of the following:

### A. Quick install (curl | bash)

Downloads a prebuilt binary to `~/.local/bin/` and links it into Herdr:

```bash
curl -fsSL https://raw.githubusercontent.com/beyondlex/herdr-recent-navigator/main/install.sh | bash
```

> **Recommendation:** Use this method — no Rust toolchain required.

### B. Install via Herdr plugin manager

```bash
herdr plugin install beyondlex/herdr-recent-navigator
```

Herdr clones the repo, builds from source, and registers the plugin
automatically.

### C. Build from source (manual)

```bash
git clone https://github.com/beyondlex/herdr-recent-navigator
cd herdr-recent-navigator
cargo build --release
herdr plugin link "$PWD"
```

## Upgrade

| Current install method | Upgrade command |
|---|---|
| curl \| bash | Re-run the curl command |
| `herdr plugin install` | `herdr plugin uninstall beyondlex.herdr-recent-navigator && herdr plugin install beyondlex/herdr-recent-navigator` |
| Build from source | `git pull && cargo build --release && herdr plugin unlink beyondlex.herdr-recent-navigator && herdr plugin link "$PWD"` |

## Bind a shortcut

Add to your Herdr config:

```toml
[[keys.command]]
key = "cmd+e"
type = "plugin_action"
command = "beyondlex.herdr-recent-navigator.focus-workspaces"
description = "Open Navigator: Workspace"


# Optional: Focus Tabs/Panes/Agents when open navigator
[[keys.command]]
key = "cmd+i"
type = "plugin_action"
command = "beyondlex.herdr-recent-navigator.focus-tabs"
description = "Open Navigator: Tab"

[[keys.command]]
key = "prefix+u"
type = "plugin_action"
command = "beyondlex.herdr-recent-navigator.focus-panes"
description = "Open Navigator: Pane"

[[keys.command]]
key = "prefix+o"
type = "plugin_action"
command = "beyondlex.herdr-recent-navigator.focus-agents"
description = "Open Navigator: Agent"
```

Reload:

```bash
herdr server reload-config
```

Press the shortcut to open the navigator popup.

### Quick-focus: jump to previous tab/pane/agent without opening the UI

Three plugin actions focus the most recently focused tab, pane, or agent directly
via MRU history, no dialog needed:

```toml
[[keys.command]]
key = "prefix+t"
type = "plugin_action"
command = "beyondlex.herdr-recent-navigator.focus-previous-tab"
description = "Jump to previous tab"

[[keys.command]]
key = "cmd+y"
type = "plugin_action"
command = "beyondlex.herdr-recent-navigator.focus-previous-pane"
description = "Jump to previous pane"

[[keys.command]]
key = "prefix+a"
type = "plugin_action"
command = "beyondlex.herdr-recent-navigator.focus-previous-agent"
description = "Jump to previous agent"
```

The tab and pane actions use the second MRU entry, mirroring GNU screen's
alt-tab workflow. The agent action skips the currently focused agent; from a
non-agent pane it jumps to the most recently focused agent.

## Configuration

### Theme

The plugin reads its theme from the `theme` field in `herdr-plugin.toml`.
Where to find that file depends on how you installed:

| Install method | Manifest location |
|---|---|
| curl \| bash | `~/.local/share/herdr-recent-navigator/herdr-plugin.toml` |
| `herdr plugin install`  | `~/.config/herdr/plugins/github/beyondlex.herdr-recent-navigator-*/herdr-plugin.toml` |
| Build from source | `$PWD/herdr-plugin.toml` (repo root) |

Add or edit the `theme` field:

```toml
theme = "light"        # "dark" (default) or "light"
```

The navigator uses a dark TokyoNight palette by default. Set `theme = "light"`
for a light palette. Full per-theme auto-detection will be added once Herdr
sends the theme name via `HERDR_PLUGIN_CONTEXT_JSON`.

### Keybindings

All internal navigation keys are configurable via the `[keybindings]` section in
`herdr-plugin.toml`. Each action accepts a list of key strings (multiple
bindings per action).

```toml
[keybindings]
next_category = ["Tab"]
previous_category = ["S-Tab"]
move_up = ["Up", "C-p"]
move_down = ["Down", "C-n"]
select = ["Enter"]
dismiss = ["Esc"]
force_quit = ["C-c"]
backspace = ["Backspace"]
```

#### Key syntax

| Format | Meaning |
|---|---|
| `Tab`, `Up`, `Down`, `Enter`, `Esc`, `Backspace`, `Space` | Special keys |
| `S-Tab` | Shift+Tab (same as `BackTab`) |
| `a`...`z`, `0`...`9` | Literal character |
| `C-a`...`C-z` | Ctrl + character |
| `S-a`...`S-z` | Shift + character |
| `M-a`...`M-z` or `A-a`...`A-z` | Alt + character |
| `C-S-a` | Ctrl + Shift + a |
| `C-M-a` | Ctrl + Alt + a |

**Note:** Terminal support for Alt+key combinations is limited. Some
terminals send `Esc` + `key` instead of a distinct Alt+key event. Prefer
Ctrl-based combinations when possible.

#### Default bindings

| Action | Default keys | Description |
|---|---|---|
| `next_category` | `Tab` | Next category tab |
| `previous_category` | `S-Tab` | Previous category tab |
| `move_up` | `Up`, `C-p` | Move selection up |
| `move_down` | `Down`, `C-n` | Move selection down |
| `select` | `Enter` | Focus selected item |
| `dismiss` | `Esc` | Clear search / close |
| `force_quit` | `C-c` | Close without focusing |
| `backspace` | `Backspace` | Delete last search character |

### Tab order and visibility

The order of the top-level category tabs — and which tabs appear at all — is
configured with a single array in `herdr-plugin.toml`: position is display
order, and a tab left out of the list is hidden entirely.

```toml
[navigator]
tabs = ["workspaces", "tabs", "panes", "agents", "all"]
```

- Valid names: `workspaces`, `tabs`, `panes`, `agents`, `all`
- Unknown names are ignored; duplicates collapse to the first occurrence
- At least one tab is always kept — an empty (or all-invalid) list falls back
  to `all` only
- `others` is accepted as a legacy alias for `all`

## Usage

| Key (default) | Action |
|---|---|
| `↑` / `↓` or `Ctrl+P` / `Ctrl+N` | Navigate list |
| `Tab` / `Shift+Tab` | Cycle category tabs |
| `Enter` | Focus selected item |
| `Esc` | Clear search / close |
| `Ctrl+C` | Close without focusing |
| Type any text | Fuzzy-search the list |

All keys in the table above are configurable — see [Keybindings](#keybindings)
to customize.

### Category tabs

- **Workspaces**: MRU workspaces with dot indicators for agent status
- **Tabs**: MRU tabs within those workspaces
- **Agents**: AI agents sorted by last activity
- **Panes**: Individual terminal panes
- **All**: find panes by runtime state — ssh target, foreground command,
  cwd — and by buffer content: any query also substring-matches pane
  scrollback and shows a one-line excerpt around each hit. A filter prefix
  narrows the search to one source and shows as a badge next to the input:
  type `cmd `, `ssh `, `cwd `, `file `, or `term ` (label + space), or `.`
  for buffer content only (`.` shorthand: `.file` = file buffers only).
  `ws `, `tab ` and `pane ` swap the list to the workspace, tab or pane
  list — type a name to fuzzy-filter down to it (e.g. `ws auth` = workspaces
  matching "auth", `pane nvim` = panes matching "nvim").


## License

MIT

