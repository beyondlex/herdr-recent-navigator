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
/// record represents. Not identity — `cmd`/`ssh`/`cwd`/`file` describe what
/// the pane is doing/where it is/what it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtherSource {
    /// Among the running foreground command (detail = the command line).
    Cmd,
    /// Active ssh/mosh login target (detail = `user@host` or `host:port`).
    Ssh,
    /// Terminal/command-output buffer match (detail = a one-line excerpt
    /// around the hit). The pane is NOT editing a file.
    Terminal,
    /// Editor buffer match (detail = a one-line excerpt around the hit).
    /// The pane's foreground is an editor, so the excerpt is file content.
    File,
    /// The pane's current directory.
    Cwd,
}

impl OtherSource {
    /// All variants, in display-priority order.
    pub const ALL: [OtherSource; 5] = [
        OtherSource::Ssh,
        OtherSource::Cmd,
        OtherSource::Terminal,
        OtherSource::File,
        OtherSource::Cwd,
    ];

    /// Short label shown in the Type column / searchable text.
    pub fn label(&self) -> &'static str {
        match self {
            OtherSource::Cmd => "cmd",
            OtherSource::Ssh => "ssh",
            OtherSource::Terminal => "term",
            OtherSource::File => "file",
            OtherSource::Cwd => "cwd",
        }
    }

    /// Ordering priority within a pane's records (lower sorts first).
    pub fn priority(&self) -> u8 {
        match self {
            OtherSource::Ssh => 0,
            OtherSource::Cmd => 1,
            OtherSource::Terminal => 2,
            OtherSource::File => 3,
            OtherSource::Cwd => 4,
        }
    }
}

/// A narrowed-search filter for the Others tab, parsed off the query prefix
/// and shown as a badge next to the input (`cmd `, `ws `, `.`, …).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum OthersFilter {
    /// Leading `.` — buffer content rows only (file + term).
    Content,
    /// `<label> ` prefix — rows of that source only.
    Source(OtherSource),
    /// Leading `ws ` — rows whose workspace matches the remaining text.
    Workspace,
    /// Leading `tab ` — rows whose tab matches the remaining text.
    Tab,
}

impl OthersFilter {
    /// Split a leading filter off `query`, returning it with the remaining
    /// text. A filter activates only when its exact label is followed by a
    /// space (`cmd `, `ssh `, `cwd `, `file `, `term `, `ws `, `tab `) — a
    /// bare label stays ordinary search text.
    pub fn parse(query: &str) -> (Option<Self>, &str) {
        let (base, rest) = match query.strip_prefix('.') {
            Some(r) => (Some(OthersFilter::Content), r),
            None => (None, query),
        };
        for src in OtherSource::ALL {
            if let Some(tail) = rest.strip_prefix(src.label())
                && let Some(needle) = tail.strip_prefix(' ')
            {
                return (Some(OthersFilter::Source(src)), needle);
            }
        }
        for (label, f) in [("ws", OthersFilter::Workspace), ("tab", OthersFilter::Tab)] {
            if let Some(tail) = rest.strip_prefix(label)
                && let Some(needle) = tail.strip_prefix(' ')
            {
                return (Some(f), needle);
            }
        }
        (base, rest)
    }

    /// Short label shown in the badge next to the input.
    pub fn label(&self) -> &'static str {
        match self {
            OthersFilter::Content => "content",
            OthersFilter::Source(src) => src.label(),
            OthersFilter::Workspace => "ws",
            OthersFilter::Tab => "tab",
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
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CategoryTab {
    Workspaces,
    Tabs,
    Agents,
    Panes,
    /// Pane state search (cmd / ssh / cwd) and pane-buffer content search in
    /// one list; a leading `.` narrows to buffer content only. Excludes agent
    /// panes.
    Others,
}

impl CategoryTab {
    /// Number of variants, for cycling.
    pub const COUNT: usize = 5;

    /// Return all variants in the default order.
    pub fn all() -> [CategoryTab; Self::COUNT] {
        [
            CategoryTab::Workspaces,
            CategoryTab::Tabs,
            CategoryTab::Panes,
            CategoryTab::Agents,
            CategoryTab::Others,
        ]
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

/// Parse the configured tab list. The array carries both order and
/// visibility: position is display order, membership is shown-at-all.
/// Unknown labels are ignored and duplicates deduped (first wins); an empty
/// or all-invalid list degrades to `["others"]` — the navigator must always
/// show at least one tab.
pub fn parse_tabs(spec: &[String]) -> Vec<CategoryTab> {
    let mut tabs: Vec<CategoryTab> = Vec::new();
    for s in spec {
        if let Ok(t) = s.trim().parse::<CategoryTab>()
            && !tabs.contains(&t)
        {
            tabs.push(t);
        }
    }
    if tabs.is_empty() {
        vec![CategoryTab::Others]
    } else {
        tabs
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
        /// The matched/runtime value: command line, ssh target, cwd path,
        /// or a content-search excerpt.
        detail: String,
        /// Where the record lives, shown in the Context column: the edited
        /// file path (`file`), the pane's cwd (`cmd`), the connected host
        /// (`ssh`), or `-` when not applicable (`cwd`/`term`).
        context: String,
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
    /// Configured category tabs — display order and visibility combined
    /// (parsed from `[navigator] tabs`). Always non-empty.
    pub tabs: Vec<CategoryTab>,
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
    /// Cached ANSI-stripped pane buffers backing the Others tab's content rows.
    pub contents: HashMap<String, String>,
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
    fn test_parse_tabs_full_list_is_passthrough() {
        // (A missing/empty `[navigator]` section never reaches parse_tabs:
        // the manifest loader substitutes the full default list first.)
        let all: Vec<String> = CategoryTab::all()
            .iter()
            .map(|t| t.label().to_lowercase())
            .collect();
        assert_eq!(parse_tabs(&all), CategoryTab::all().to_vec());
    }

    #[test]
    fn test_parse_tabs_reorders_and_hides() {
        let spec = vec!["others".to_string(), "workspaces".to_string()];
        assert_eq!(
            parse_tabs(&spec),
            vec![CategoryTab::Others, CategoryTab::Workspaces]
        );
    }

    #[test]
    fn test_parse_tabs_dedupes_and_drops_unknown() {
        let spec = vec![
            "agents".to_string(),
            "nope".to_string(),
            " agents ".to_string(),
            "others".to_string(),
        ];
        assert_eq!(
            parse_tabs(&spec),
            vec![CategoryTab::Agents, CategoryTab::Others]
        );
    }

    #[test]
    fn test_parse_tabs_empty_or_all_invalid_keeps_others() {
        // Nothing configured to show: the navigator must keep one tab.
        assert_eq!(parse_tabs(&[]), vec![CategoryTab::Others]);
        assert_eq!(parse_tabs(&["nope".to_string()]), vec![CategoryTab::Others]);
    }

    // ── OthersFilter::parse ──

    #[test]
    fn test_others_filter_label_plus_space_activates() {
        assert_eq!(
            OthersFilter::parse("cmd deploy"),
            (Some(OthersFilter::Source(OtherSource::Cmd)), "deploy")
        );
        assert_eq!(
            OthersFilter::parse("ssh "),
            (Some(OthersFilter::Source(OtherSource::Ssh)), "")
        );
        assert_eq!(
            OthersFilter::parse("cwd /var/log"),
            (Some(OthersFilter::Source(OtherSource::Cwd)), "/var/log")
        );
        assert_eq!(
            OthersFilter::parse("ws auth"),
            (Some(OthersFilter::Workspace), "auth")
        );
        assert_eq!(OthersFilter::parse("tab "), (Some(OthersFilter::Tab), ""));
        assert_eq!(
            OthersFilter::parse("tab main"),
            (Some(OthersFilter::Tab), "main")
        );
    }

    #[test]
    fn test_others_filter_bare_label_stays_search_text() {
        // No trailing space: the label is ordinary fuzzy text.
        assert_eq!(OthersFilter::parse("cmd"), (None, "cmd"));
        assert_eq!(OthersFilter::parse("ws"), (None, "ws"));
        assert_eq!(OthersFilter::parse("tab"), (None, "tab"));
        // A label-shaped prefix that is not exactly the label is plain text.
        assert_eq!(OthersFilter::parse("cmdx rest"), (None, "cmdx rest"));
        assert_eq!(OthersFilter::parse("tabx rest"), (None, "tabx rest"));
        // A label in the middle of the query never activates a filter.
        assert_eq!(OthersFilter::parse("deploy cmd"), (None, "deploy cmd"));
        assert_eq!(OthersFilter::parse("deploy ws"), (None, "deploy ws"));
    }

    #[test]
    fn test_others_filter_ws_tab_not_source_aliases() {
        // `term` is a source label, `tab` is a dimension filter: distinct.
        assert_eq!(
            OthersFilter::parse("term "),
            (Some(OthersFilter::Source(OtherSource::Terminal)), "")
        );
        assert_eq!(OthersFilter::parse("tab "), (Some(OthersFilter::Tab), ""));
        // Source labels still win over ws/tab.
        assert_eq!(
            OthersFilter::parse("cmd deploy"),
            (Some(OthersFilter::Source(OtherSource::Cmd)), "deploy")
        );
    }

    #[test]
    fn test_others_filter_labels() {
        assert_eq!(OthersFilter::Content.label(), "content");
        assert_eq!(OthersFilter::Workspace.label(), "ws");
        assert_eq!(OthersFilter::Tab.label(), "tab");
        assert_eq!(
            OthersFilter::Source(OtherSource::Ssh).label(),
            OtherSource::Ssh.label()
        );
    }

    #[test]
    fn test_others_filter_dot_and_dot_label_combo() {
        assert_eq!(
            OthersFilter::parse(".deploy"),
            (Some(OthersFilter::Content), "deploy")
        );
        // `.file ` narrows past content down to the file source.
        assert_eq!(
            OthersFilter::parse(".file todo"),
            (Some(OthersFilter::Source(OtherSource::File)), "todo")
        );
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
        assert_eq!(OtherSource::Terminal.label(), "term");
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
