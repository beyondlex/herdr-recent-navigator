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
- **Follows your Herdr theme**: dark (TokyoNight) or light (One Light) palettes, or the host terminal palette when Herdr is set to `terminal`, with your `[theme.custom]` colour overrides applied on top
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

User settings live in `config.toml` inside the plugin's config directory, which
Herdr keeps separate from the plugin files so upgrades never overwrite it:

```bash
herdr plugin config-dir beyondlex.herdr-recent-navigator
# usually ~/.config/herdr/plugins/config/beyondlex.herdr-recent-navigator
```

Create `config.toml` there (the installer seeds a commented template if the
file doesn't exist). `theme`, `[keybindings]` and `[navigator]` all go in this one file:

```toml
theme = "terminal"

[keybindings]
move_up = ["Up", "C-k"]
move_down = ["Down", "C-j"]
```

Settings still in `herdr-plugin.toml` (the old location) are honored as a
fallback, but the installer regenerates that file on every upgrade, so move
anything you've customized into `config.toml`.

### Theme

```toml
theme = "terminal"     # fallback: "terminal" | "dark" | "light"
```

The navigator follows your Herdr theme. The active theme name is resolved from
Herdr's `HERDR_PLUGIN_CONTEXT_JSON` when available, then from `[theme] name` in
your Herdr config, then from this setting:

- `terminal` — inherit the terminal's own colours (default foreground/background
  plus ANSI accents). Use this when Herdr is configured with
  `[theme] name = "terminal"`.
- a light theme name (`*-light`, `*-latte`, `*-day`, `*-dawn`, `*-lotus`) — the
  built-in One Light palette.
- anything else, including an unset theme — the built-in dark (TokyoNight)
  palette, so the popup matches Herdr's UI rather than the host terminal.

Herdr does not currently expose theme colours to plugins, so a Herdr theme the
navigator has no dedicated palette for is approximated by the built-in dark or
light palette.

Colour overrides from Herdr's `[theme.custom]` table are applied on top of the
palette, in the same order Herdr uses (built-in theme, then `custom`). A custom
`accent`, `panel_bg`, `surface0`, `text`, `selection_bg`, and so on are therefore
respected. Values accept hex, `rgb(r,g,b)`, reset aliases (`reset`, `default`,
`none`, `transparent`) and common named colours. `panel_bg` maps to the popup
background and to the accent-background text (the active tab and filter chips);
it is the colour Herdr fills the plugin-popup frame with, even though Herdr's
config reference only shows it in examples. `selection_bg` / `active_row_bg` map
to the selected row. `sidebar_bg` is desktop-sidebar-only and has no popup
equivalent, so it is not mapped. Herdr's `surface_dim`
(separators, scrollbars) and `surface1` (dragged rows) are not mapped. The
`[theme.custom.light]` / `[theme.custom.dark]` sub-tables are ignored, because a
plugin is never told the current light/dark appearance.

### Keybindings

All internal navigation keys are configurable via the `[keybindings]` section.
Each action accepts a list of key strings (multiple bindings per action).

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
| `dismiss` | `Esc` | Leave filter input, or close from navigation |
| `force_quit` | `C-c` | Close without focusing |
| `backspace` | `Backspace` | Delete last query character while filtering |

### Tab order and visibility

The order of the top-level category tabs — and which tabs appear at all — is
configured with a single array in the plugin's `config.toml` (see
[Configuration](#configuration)): position is display order, and a tab left
out of the list is hidden entirely. Like `theme` and `[keybindings]`,
`herdr-plugin.toml` is only read as a fallback when `config.toml` has no
`[navigator]` section — the installer regenerates the manifest on upgrade.

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
| `/` | Enter filter mode without changing the current query |
| `j` / `k`, `↑` / `↓`, or `Ctrl+N` / `Ctrl+P` | Navigate the list outside filter mode |
| Type text or use `Backspace` | Edit the query while filtering; printable keys do nothing in navigation mode |
| `Esc` | Leave filter mode preserving query/results; press again in navigation mode to close |
| `Tab` / `Shift+Tab` | Cycle category tabs in either mode |
| `Enter` | Focus selected item |
| `Ctrl+C` | Close without focusing |

Arrow, Ctrl navigation, category, select, dismiss and backspace bindings are
configurable — see [Keybindings](#keybindings). Filter mode gives printable
characters priority over custom printable movement bindings so text remains
editable.

### Category tabs

- **Workspaces**: MRU workspaces with dot indicators for agent status.
  Linked git worktrees show as `<repo> ⎇ <worktree>` directly under their
  main-checkout workspace, and typing the repo name finds them
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

