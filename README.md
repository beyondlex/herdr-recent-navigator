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

> **Recommendation:** Use the curl method — no Rust toolchain required.

### Quick install (curl | bash)

```bash
curl -fsSL https://raw.githubusercontent.com/beyondlex/herdr-recent-navigator/main/install.sh | bash
```

Downloads a prebuilt binary for your platform to `~/.local/bin/` and links it
into Herdr.

### Install via Herdr plugin manager

```bash
herdr plugin install beyondlex/herdr-recent-navigator
```

Herdr clones the repo, builds from source, and registers the plugin
automatically. Equivalent to the build-from-source steps below, but
orchestrated by Herdr to a preset location.

### Build from source (manual)

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

Press the shortcut to open the navigator popup.

## Configuration

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

## Usage

| Key | Action |
|---|---|
| `↑` / `↓` | Navigate list |
| `Tab` / `Shift+Tab` | Cycle category tabs |
| `Enter` | Focus selected item |
| `Esc` | Clear search / close |
| `Ctrl+C` | Close without focusing |
| Type any text | Fuzzy-search the list |

### Category tabs

- **Workspaces**: MRU workspaces with dot indicators for agent status
- **Tabs**: MRU tabs within those workspaces
- **Agents**: AI agents sorted by last activity
- **Panes**: Individual terminal panes


## License

MIT

