use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};

/// The status of an AI agent within a pane.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    Working,
    Blocked,
    Done,
    Idle,
    /// Normal pane, no running AI agent.
    None,
}

impl AgentStatus {
    /// Returns true if the agent is actively doing work (Working status).
    /// Currently unused after ActiveOnly filter removal; kept for future use.
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        matches!(self, AgentStatus::Working)
    }
}

/// A composite navigation node representing a pane with its workspace/tab/agent context.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NavigationNode {
    pub workspace_id: String,
    pub workspace_name: String,
    pub tab_id: String,
    pub tab_name: String,
    pub pane_id: String,
    pub pane_name: Option<String>,
    pub agent_id: Option<String>,
    pub agent_status: AgentStatus,
    /// Millisecond timestamp for MRU sorting.
    pub last_accessed_at: u64,
}

/// The source kind of an "Others" row: which runtime state dimension a pane
/// record represents. Not identity — `cmd`/`ssh`/`cwd` describe what the pane
/// is doing/where it is. `File` is reserved for the deferred title/statusline
/// parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtherSource {
    /// Among the running foreground command (detail = the command line).
    Cmd,
    /// Active ssh/mosh login target (detail = `user@host` or `host:port`).
    Ssh,
    /// A file the pane is currently editing (deferred — detail = file path).
    #[allow(dead_code)]
    File,
    /// The pane's current directory.
    Cwd,
}

impl OtherSource {
    /// Short label shown in the Type column / searchable text.
    pub fn label(&self) -> &'static str {
        match self {
            OtherSource::Cmd => "cmd",
            OtherSource::Ssh => "ssh",
            OtherSource::File => "file",
            OtherSource::Cwd => "cwd",
        }
    }

    /// Ordering priority within a pane's records (lower sorts first).
    pub fn priority(&self) -> u8 {
        match self {
            OtherSource::Ssh => 0,
            OtherSource::Cmd => 1,
            OtherSource::File => 2,
            OtherSource::Cwd => 3,
        }
    }
}

/// Lazily-fetched runtime state for a pane, used by the Others tab.
/// Unlike `NavigationNode` (refreshed every 2s), this is refetched only while
/// the Others tab is active, and only once per pane per refresh window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneOthers {
    pub cwd: Option<String>,
    pub command: Option<String>,
    pub ssh_target: Option<String>,
}

/// The category tabs at the top of the navigator UI.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum CategoryTab {
    Workspaces,
    Tabs,
    Agents,
    Panes,
    /// Pane runtime state search (cmd / ssh / cwd), excluding agent panes.
    Others,
}

impl CategoryTab {
    /// Number of variants, for cycling.
    pub const COUNT: usize = 5;

    /// Return all variants in order.
    pub fn all() -> [CategoryTab; Self::COUNT] {
        [
            CategoryTab::Workspaces,
            CategoryTab::Tabs,
            CategoryTab::Panes,
            CategoryTab::Agents,
            CategoryTab::Others,
        ]
    }

    /// Move to the next tab (wrapping).
    pub fn next(&self) -> Self {
        match self {
            CategoryTab::Workspaces => CategoryTab::Tabs,
            CategoryTab::Tabs => CategoryTab::Panes,
            CategoryTab::Panes => CategoryTab::Agents,
            CategoryTab::Agents => CategoryTab::Others,
            CategoryTab::Others => CategoryTab::Workspaces,
        }
    }

    /// Move to the previous tab (wrapping).
    pub fn previous(&self) -> Self {
        match self {
            CategoryTab::Workspaces => CategoryTab::Others,
            CategoryTab::Tabs => CategoryTab::Workspaces,
            CategoryTab::Panes => CategoryTab::Tabs,
            CategoryTab::Agents => CategoryTab::Panes,
            CategoryTab::Others => CategoryTab::Agents,
        }
    }

    /// Display label for the tab.
    pub fn label(&self) -> &'static str {
        match self {
            CategoryTab::Workspaces => "Workspaces",
            CategoryTab::Tabs => "Tabs",
            CategoryTab::Agents => "Agents",
            CategoryTab::Panes => "Panes",
            CategoryTab::Others => "Others",
        }
    }
}

/// A display item representing one row in the category-specific list.
/// Each variant carries only the fields relevant to its tab's rendering.
#[derive(Debug, Clone)]
pub enum DisplayItem {
    Workspace {
        name: String,
        id: String,
        pane_ids: Vec<String>,
        agent_statuses: Vec<AgentStatus>,
        last_accessed_at: u64,
    },
    Tab {
        name: String,
        workspace: String,
        tab_id: String,
        pane_ids: Vec<String>,
        agent_statuses: Vec<AgentStatus>,
        last_accessed_at: u64,
    },
    Agent {
        agent_id: String,
        status: AgentStatus,
        pane_id: String,
        tab: String,
        workspace: String,
        last_accessed_at: u64,
    },
    Pane {
        pane_id: String,
        pane_name: String,
        tab: String,
        workspace: String,
        agent_id: Option<String>,
        status: AgentStatus,
        last_accessed_at: u64,
    },
    Other {
        pane_id: String,
        pane_name: String,
        tab: String,
        workspace: String,
        source: OtherSource,
        /// The matched/runtime value: command line, ssh target, or cwd path.
        detail: String,
        last_accessed_at: u64,
    },
}

impl DisplayItem {
    /// Deterministic secondary sort key for stable ordering when timestamps tie.
    pub fn sort_key(&self) -> String {
        match self {
            DisplayItem::Workspace { name, .. } => name.clone(),
            DisplayItem::Tab {
                name, workspace, ..
            } => format!("{}:{}", workspace, name),
            DisplayItem::Agent { agent_id, .. } => agent_id.clone(),
            DisplayItem::Pane { pane_name, .. } => pane_name.clone(),
            DisplayItem::Other {
                pane_name, source, ..
            } => format!("{}{}", source.priority(), pane_name),
        }
    }

    /// Build the searchable text for this item, used for fuzzy matching.
    pub fn search_text(&self) -> String {
        match self {
            DisplayItem::Workspace { name, .. } => name.clone(),
            DisplayItem::Tab {
                name, workspace, ..
            } => format!("{} {}", name, workspace),
            DisplayItem::Agent {
                agent_id,
                tab,
                workspace,
                ..
            } => format!("{} {} {}", agent_id, tab, workspace),
            DisplayItem::Pane {
                pane_name,
                tab,
                workspace,
                agent_id,
                ..
            } => {
                format!(
                    "{} {} {} {}",
                    pane_name,
                    tab,
                    workspace,
                    agent_id.as_deref().unwrap_or("")
                )
            }
            DisplayItem::Other {
                pane_name,
                tab,
                workspace,
                source,
                detail,
                ..
            } => {
                format!(
                    "{} {} {} {} {}",
                    source.label(),
                    detail,
                    pane_name,
                    tab,
                    workspace
                )
            }
        }
    }
}

/// What entity to focus when the user selects an item and exits.
#[derive(Debug, Clone)]
pub enum FocusTarget {
    Workspace(String),
    Tab(String),
    Pane(String),
}

/// Result of a key event handling.
#[derive(Debug, PartialEq, Eq)]
pub enum KeyAction {
    /// Continue the event loop.
    Continue,
    /// Exit without focusing any workspace (Esc dismiss).
    ExitDismiss,
    /// Exit and focus the selected workspace (Enter / number keys).
    ExitSelect,
}

/// A parsed key combination like `C-S-n` (Ctrl+Shift+n) or `Tab`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl std::str::FromStr for KeyBinding {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut modifiers = KeyModifiers::NONE;
        let mut rest = s;

        loop {
            if let Some(tail) = rest.strip_prefix("C-") {
                modifiers |= KeyModifiers::CONTROL;
                rest = tail;
            } else if let Some(tail) = rest.strip_prefix("S-") {
                modifiers |= KeyModifiers::SHIFT;
                rest = tail;
            } else if let Some(tail) = rest.strip_prefix("M-").or_else(|| rest.strip_prefix("A-")) {
                modifiers |= KeyModifiers::ALT;
                rest = tail;
            } else {
                break;
            }
        }

        let code = match rest {
            "Tab" => {
                if modifiers.contains(KeyModifiers::SHIFT) {
                    // Shift+Tab produces BackTab in crossterm
                    KeyCode::BackTab
                } else {
                    KeyCode::Tab
                }
            }
            "BackTab" => KeyCode::BackTab,
            "Up" => KeyCode::Up,
            "Down" => KeyCode::Down,
            "Enter" => KeyCode::Enter,
            "Esc" => KeyCode::Esc,
            "Backspace" => KeyCode::Backspace,
            "Space" => KeyCode::Char(' '),
            c if c.len() == 1 => KeyCode::Char(c.chars().next().unwrap()),
            _ => return Err(format!("Unknown key: {s}")),
        };

        Ok(KeyBinding { code, modifiers })
    }
}

/// Logical actions that can be bound to keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    NextCategory,
    PreviousCategory,
    MoveUp,
    MoveDown,
    Select,
    Dismiss,
    ForceQuit,
    Backspace,
}

/// Configurable keybinding map. Each action accepts multiple key strings.
/// Parsed from `[keybindings]` section in `herdr-plugin.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keybindings {
    #[serde(default = "default_next_category")]
    pub next_category: Vec<String>,
    #[serde(default = "default_previous_category")]
    pub previous_category: Vec<String>,
    #[serde(default = "default_move_up")]
    pub move_up: Vec<String>,
    #[serde(default = "default_move_down")]
    pub move_down: Vec<String>,
    #[serde(default = "default_select")]
    pub select: Vec<String>,
    #[serde(default = "default_dismiss")]
    pub dismiss: Vec<String>,
    #[serde(default = "default_force_quit")]
    pub force_quit: Vec<String>,
    #[serde(default = "default_backspace")]
    pub backspace: Vec<String>,
}

fn default_next_category() -> Vec<String> {
    vec!["Tab".into()]
}
fn default_previous_category() -> Vec<String> {
    vec!["S-Tab".into()]
}
fn default_move_up() -> Vec<String> {
    vec!["Up".into(), "C-p".into()]
}
fn default_move_down() -> Vec<String> {
    vec!["Down".into(), "C-n".into()]
}
fn default_select() -> Vec<String> {
    vec!["Enter".into()]
}
fn default_dismiss() -> Vec<String> {
    vec!["Esc".into()]
}
fn default_force_quit() -> Vec<String> {
    vec!["C-c".into()]
}
fn default_backspace() -> Vec<String> {
    vec!["Backspace".into()]
}

impl Default for Keybindings {
    fn default() -> Self {
        Self {
            next_category: default_next_category(),
            previous_category: default_previous_category(),
            move_up: default_move_up(),
            move_down: default_move_down(),
            select: default_select(),
            dismiss: default_dismiss(),
            force_quit: default_force_quit(),
            backspace: default_backspace(),
        }
    }
}

impl Keybindings {
    /// Return the action triggered by a given key event, or `None` if no binding matches.
    pub fn action_for(&self, key: &KeyEvent) -> Option<Action> {
        let binding = KeyBinding {
            code: key.code,
            modifiers: key.modifiers,
        };
        macro_rules! check {
            ($field:ident, $action:expr) => {
                if self
                    .$field
                    .iter()
                    .any(|s| s.parse::<KeyBinding>().ok().as_ref() == Some(&binding))
                {
                    return Some($action);
                }
            };
        }
        check!(next_category, Action::NextCategory);
        check!(previous_category, Action::PreviousCategory);
        check!(move_up, Action::MoveUp);
        check!(move_down, Action::MoveDown);
        check!(select, Action::Select);
        check!(dismiss, Action::Dismiss);
        check!(force_quit, Action::ForceQuit);
        check!(backspace, Action::Backspace);
        None
    }
}

/// Global TUI application state.
pub struct AppState {
    /// Configurable keybindings.
    pub keybindings: Keybindings,
    /// Full list of navigation nodes.
    pub nodes: Vec<NavigationNode>,
    /// Currently selected category tab.
    pub current_category: CategoryTab,
    /// Search input text.
    pub search_query: String,
    /// Currently highlighted list index.
    pub selected_index: usize,
    /// Animation tick for spinner (incremented each render frame).
    pub spinner_tick: u32,
    /// Herdr theme name (e.g. "tokyonight", "tokyonight-storm") from context.
    pub theme_name: Option<String>,
    /// Cache key: hash of the last display-list build inputs.
    /// Used to skip re-sorting every frame when nothing changed.
    pub cache_key: Option<u64>,
    /// Cached display list (already searched/filtered).
    pub cached_displayed: Rc<Vec<DisplayItem>>,
    /// Total items before search filtering; shown in the status bar count.
    pub cached_total: usize,
    /// Lazily-fetched pane runtime state for the Others tab (cmd/ssh/cwd).
    pub others: HashMap<String, PaneOthers>,
}

fn state_file_path() -> PathBuf {
    crate::tracker::state_dir_or_default().join("state.json")
}

impl AppState {
    /// Persist the current category to a temp file so it survives restarts.
    pub fn save_last_category(&self) {
        if let Ok(data) = serde_json::to_string(&self.current_category.label())
            && let Err(e) = std::fs::write(state_file_path(), data)
        {
            log::error!("Failed to save last category: {e}");
        }
    }

    /// Load the last-used category from the temp file (if any).
    pub fn load_last_category() -> Option<CategoryTab> {
        let data = std::fs::read_to_string(state_file_path()).ok()?;
        let label = data.trim().trim_matches('"');
        CategoryTab::all()
            .iter()
            .find(|t| t.label() == label)
            .cloned()
    }

    /// Save an arbitrary category tab to the state file.
    /// Used by --quick-focus to pre-select the tab before the navigator pane opens.
    pub fn save_category(&self, cat: &CategoryTab) {
        if let Ok(data) = serde_json::to_string(cat.label())
            && let Err(e) = std::fs::write(state_file_path(), data)
        {
            log::error!("Failed to save category: {e}");
        }
    }
}

impl std::str::FromStr for CategoryTab {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "workspaces" => Ok(CategoryTab::Workspaces),
            "tabs" => Ok(CategoryTab::Tabs),
            "agents" => Ok(CategoryTab::Agents),
            "panes" => Ok(CategoryTab::Panes),
            "others" => Ok(CategoryTab::Others),
            _ => Err(format!("Unknown category tab: {s}")),
        }
    }
}

#[cfg(test)]
mod category_tab_tests {
    use super::*;

    #[test]
    fn test_category_tab_from_str_workspaces() {
        assert_eq!(
            "workspaces".parse::<CategoryTab>().unwrap(),
            CategoryTab::Workspaces
        );
    }

    #[test]
    fn test_category_tab_from_str_tabs() {
        assert_eq!("tabs".parse::<CategoryTab>().unwrap(), CategoryTab::Tabs);
    }

    #[test]
    fn test_category_tab_from_str_agents() {
        assert_eq!(
            "agents".parse::<CategoryTab>().unwrap(),
            CategoryTab::Agents
        );
    }

    #[test]
    fn test_category_tab_from_str_panes() {
        assert_eq!("panes".parse::<CategoryTab>().unwrap(), CategoryTab::Panes);
    }

    #[test]
    fn test_category_tab_from_str_others() {
        assert_eq!(
            "others".parse::<CategoryTab>().unwrap(),
            CategoryTab::Others
        );
    }

    #[test]
    fn test_category_tab_from_str_invalid() {
        assert!("invalid".parse::<CategoryTab>().is_err());
    }

    #[test]
    fn test_category_tab_from_str_case_sensitive() {
        assert!("Workspaces".parse::<CategoryTab>().is_err());
    }
}

#[cfg(test)]
mod other_source_tests {
    use super::*;

    #[test]
    fn test_other_source_labels() {
        assert_eq!(OtherSource::Ssh.label(), "ssh");
        assert_eq!(OtherSource::Cmd.label(), "cmd");
        assert_eq!(OtherSource::Cwd.label(), "cwd");
        assert_eq!(OtherSource::File.label(), "file");
    }

    #[test]
    fn test_other_source_priority_order() {
        assert!(OtherSource::Ssh.priority() < OtherSource::Cmd.priority());
        assert!(OtherSource::Cmd.priority() < OtherSource::File.priority());
        assert!(OtherSource::File.priority() < OtherSource::Cwd.priority());
    }
}

#[cfg(test)]
mod agent_status_tests {
    use super::*;

    #[test]
    fn test_agent_status_is_active_working() {
        assert!(AgentStatus::Working.is_active());
    }

    #[test]
    fn test_agent_status_is_active_done() {
        assert!(!AgentStatus::Done.is_active());
    }

    #[test]
    fn test_agent_status_is_active_blocked() {
        assert!(!AgentStatus::Blocked.is_active());
    }

    #[test]
    fn test_agent_status_is_active_idle() {
        assert!(!AgentStatus::Idle.is_active());
    }

    #[test]
    fn test_agent_status_is_active_none() {
        assert!(!AgentStatus::None.is_active());
    }
}
