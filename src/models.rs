use std::path::PathBuf;
use std::rc::Rc;

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

/// The category tabs at the top of the navigator UI.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum CategoryTab {
    Workspaces,
    Tabs,
    Agents,
    Panes,
}

impl CategoryTab {
    /// Number of variants, for cycling.
    pub const COUNT: usize = 4;

    /// Return all variants in order.
    pub fn all() -> [CategoryTab; Self::COUNT] {
        [
            CategoryTab::Workspaces,
            CategoryTab::Tabs,
            CategoryTab::Panes,
            CategoryTab::Agents,
        ]
    }

    /// Move to the next tab (wrapping).
    pub fn next(&self) -> Self {
        match self {
            CategoryTab::Workspaces => CategoryTab::Tabs,
            CategoryTab::Tabs => CategoryTab::Panes,
            CategoryTab::Panes => CategoryTab::Agents,
            CategoryTab::Agents => CategoryTab::Workspaces,
        }
    }

    /// Move to the previous tab (wrapping).
    pub fn previous(&self) -> Self {
        match self {
            CategoryTab::Workspaces => CategoryTab::Agents,
            CategoryTab::Tabs => CategoryTab::Workspaces,
            CategoryTab::Panes => CategoryTab::Tabs,
            CategoryTab::Agents => CategoryTab::Panes,
        }
    }

    /// Display label for the tab.
    pub fn label(&self) -> &'static str {
        match self {
            CategoryTab::Workspaces => "Workspaces",
            CategoryTab::Tabs => "Tabs",
            CategoryTab::Agents => "Agents",
            CategoryTab::Panes => "Panes",
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

/// Global TUI application state.
pub struct AppState {
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
}

fn state_file_path() -> PathBuf {
    crate::tracker::state_dir_or_default().join("state.json")
}

impl AppState {
    /// Persist the current category to a temp file so it survives restarts.
    pub fn save_last_category(&self) {
        if let Ok(data) = serde_json::to_string(&self.current_category.label()) {
            if let Err(e) = std::fs::write(state_file_path(), data) {
                log::error!("Failed to save last category: {e}");
            }
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
        if let Ok(data) = serde_json::to_string(cat.label()) {
            if let Err(e) = std::fs::write(state_file_path(), data) {
                log::error!("Failed to save category: {e}");
            }
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
    fn test_category_tab_from_str_invalid() {
        assert!("invalid".parse::<CategoryTab>().is_err());
    }

    #[test]
    fn test_category_tab_from_str_case_sensitive() {
        assert!("Workspaces".parse::<CategoryTab>().is_err());
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
