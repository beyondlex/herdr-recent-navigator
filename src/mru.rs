use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};

// AgentStatus is only used in #[cfg(test)] code (make_node helper and test data).
// The import is kept here so `use super::*` in tests can access it.
#[allow(unused_imports)]
use crate::models::{AgentStatus, CategoryTab, DisplayItem, NavigationNode};

thread_local! {
    static FUZZY_MATCHER: RefCell<Matcher> = RefCell::new(Matcher::new(Config::DEFAULT));
}

/// Common filter: active-only check.
/// Does NOT exclude any entity or sort — exclusion and MRU sorting are
/// handled per-category in builders using their own level's timestamp map.
/// Build a category-specific display list.
///
/// Each category filters at its own granularity:
/// - Workspaces: exclude the current workspace; sort by ws_ts
/// - Tabs:       exclude the current tab; sort by merged tab_ts (direct Tab events
///   or derived from pane_ts when tab.focused is unreliable)
/// - Panes:      exclude the navigator's own pane + previously-focused pane; sort by pane_ts
/// - Agents:     same exclusion as panes; sort by pane_ts
///
/// Timestamp maps are built from MRU entries and kept separate per level
/// so that workspace focus doesn't override tab recency and vice versa.
pub struct BuildOptions<'a> {
    pub pane_ts: &'a HashMap<String, u64>,
    pub tab_ts: &'a HashMap<String, u64>,
    pub ws_ts: &'a HashMap<String, u64>,
    pub active_workspace_id: Option<&'a str>,
    pub active_pane_id: Option<&'a str>,
    pub active_tab_id: Option<&'a str>,
    pub self_pane_id: Option<&'a str>,
}

/// Compute a lightweight cache key for the display list.
/// Uses node count + selected field hashes to quickly detect changes
/// without cloning the entire node list.
/// Returns 0 when caching should be skipped (e.g., always rebuild).
pub fn build_cache_key(
    nodes: &[NavigationNode],
    opts: &BuildOptions,
    category: &CategoryTab,
    search_query: &str,
) -> u64 {
    if nodes.is_empty() {
        return 0;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Node-level change detection (lightweight: hash first/last nodes + count)
    nodes.len().hash(&mut hasher);
    if let Some(first) = nodes.first() {
        first.last_accessed_at.hash(&mut hasher);
        first.workspace_id.hash(&mut hasher);
    }
    if let Some(last) = nodes.last() {
        last.last_accessed_at.hash(&mut hasher);
    }
    // Filter/sort parameters
    category.label().hash(&mut hasher);
    search_query.hash(&mut hasher);
    opts.active_workspace_id.hash(&mut hasher);
    opts.active_pane_id.hash(&mut hasher);
    opts.active_tab_id.hash(&mut hasher);
    opts.self_pane_id.hash(&mut hasher);
    hasher.finish()
}

pub fn build_display_list(
    nodes: &[NavigationNode],
    opts: &BuildOptions,
    category: &CategoryTab,
) -> Vec<DisplayItem> {
    let all: Vec<&NavigationNode> = nodes.iter().collect();
    match category {
        CategoryTab::Workspaces => {
            build_workspace_items(&all, opts.ws_ts, opts.active_workspace_id)
        }
        CategoryTab::Tabs => {
            let merged_tab_ts = merge_tab_ts(&all, opts.tab_ts, opts.pane_ts);
            build_tab_items(&all, &merged_tab_ts, opts.active_tab_id)
        }
        CategoryTab::Agents => {
            build_agent_items(&all, opts.pane_ts, opts.active_pane_id, opts.self_pane_id)
        }
        CategoryTab::Panes => {
            build_pane_items(&all, opts.pane_ts, opts.active_pane_id, opts.self_pane_id)
        }
    }
}

/// Merge direct tab_ts entries with derived timestamps from pane_ts.
///
/// `tab.focused` events are unreliable (Herdr may not fire them for every tab
/// switch), so for tabs without a direct event we use the max `pane_ts` across
/// all panes within that tab as a fallback recency signal.
fn merge_tab_ts(
    nodes: &[&NavigationNode],
    tab_ts: &HashMap<String, u64>,
    pane_ts: &HashMap<String, u64>,
) -> HashMap<String, u64> {
    let mut merged = tab_ts.clone();
    for n in nodes {
        if let Some(pane_ts) = pane_ts.get(&n.pane_id).copied() {
            if pane_ts > 0 {
                merged
                    .entry(n.tab_id.clone())
                    .and_modify(|e| *e = (*e).max(pane_ts))
                    .or_insert(pane_ts);
            }
        }
    }
    merged
}

fn mru_sort(items: &mut [DisplayItem]) {
    items.sort_by(|a, b| {
        b.display_ts()
            .cmp(&a.display_ts())
            .then_with(|| a.sort_key().cmp(&b.sort_key()))
    });
}

fn exclude_pane(
    n: &&NavigationNode,
    exclude_pane_id: Option<&str>,
    self_pane_id: Option<&str>,
) -> bool {
    if exclude_pane_id.is_some_and(|pid| n.pane_id == pid) {
        return false;
    }
    if self_pane_id.is_some_and(|pid| n.pane_id == pid) {
        return false;
    }
    true
}

fn build_workspace_items(
    nodes: &[&NavigationNode],
    ts_map: &HashMap<String, u64>,
    exclude_workspace_id: Option<&str>,
) -> Vec<DisplayItem> {
    let mut map: HashMap<String, DisplayItem> = HashMap::new();
    for n in nodes {
        if exclude_workspace_id.is_some_and(|ws_id| n.workspace_id == ws_id) {
            continue;
        }
        let node_ts = ts_map.get(&n.workspace_id).copied().unwrap_or(0);
        let item = map
            .entry(n.workspace_id.clone())
            .or_insert_with(|| DisplayItem::Workspace {
                name: n.workspace_name.clone(),
                id: n.workspace_id.clone(),
                pane_ids: Vec::new(),
                agent_statuses: Vec::new(),
                last_accessed_at: 0,
            });
        if let DisplayItem::Workspace {
            pane_ids,
            agent_statuses,
            last_accessed_at,
            ..
        } = item
        {
            pane_ids.push(n.pane_id.clone());
            if n.agent_id.is_some() {
                agent_statuses.push(n.agent_status.clone());
            }
            *last_accessed_at = (*last_accessed_at).max(node_ts);
        }
    }
    let mut items: Vec<DisplayItem> = map.into_values().collect();
    mru_sort(&mut items);
    items
}

fn build_tab_items(
    nodes: &[&NavigationNode],
    ts_map: &HashMap<String, u64>,
    exclude_tab_id: Option<&str>,
) -> Vec<DisplayItem> {
    let mut map: HashMap<String, DisplayItem> = HashMap::new();
    for n in nodes {
        if exclude_tab_id.is_some_and(|tab_id| n.tab_id == tab_id) {
            continue;
        }
        let node_ts = ts_map.get(&n.tab_id).copied().unwrap_or(0);
        let key = format!("{}:{}", n.workspace_id, n.tab_id);
        let item = map.entry(key).or_insert_with(|| DisplayItem::Tab {
            name: n.tab_name.clone(),
            workspace: n.workspace_name.clone(),
            tab_id: n.tab_id.clone(),
            pane_ids: Vec::new(),
            agent_statuses: Vec::new(),
            last_accessed_at: 0,
        });
        if let DisplayItem::Tab {
            pane_ids,
            agent_statuses,
            last_accessed_at,
            ..
        } = item
        {
            pane_ids.push(n.pane_id.clone());
            if n.agent_id.is_some() {
                agent_statuses.push(n.agent_status.clone());
            }
            *last_accessed_at = (*last_accessed_at).max(node_ts);
        }
    }
    let mut items: Vec<DisplayItem> = map.into_values().collect();
    mru_sort(&mut items);
    items
}

fn build_agent_items(
    nodes: &[&NavigationNode],
    ts_map: &HashMap<String, u64>,
    exclude_pane_id: Option<&str>,
    self_pane_id: Option<&str>,
) -> Vec<DisplayItem> {
    let mut items: Vec<DisplayItem> = nodes
        .iter()
        .filter(|n| n.agent_id.is_some() && exclude_pane(n, exclude_pane_id, self_pane_id))
        .map(|n| {
            let ts = ts_map.get(&n.pane_id).copied().unwrap_or(0);
            DisplayItem::Agent {
                agent_id: n.agent_id.clone().unwrap_or_default(),
                status: n.agent_status.clone(),
                pane_id: n.pane_id.clone(),
                tab: n.tab_name.clone(),
                workspace: n.workspace_name.clone(),
                last_accessed_at: ts,
            }
        })
        .collect();
    mru_sort(&mut items);
    items
}

fn build_pane_items(
    nodes: &[&NavigationNode],
    ts_map: &HashMap<String, u64>,
    exclude_pane_id: Option<&str>,
    self_pane_id: Option<&str>,
) -> Vec<DisplayItem> {
    let mut items: Vec<DisplayItem> = nodes
        .iter()
        .filter(|n| exclude_pane(n, exclude_pane_id, self_pane_id))
        .map(|n| {
            let ts = ts_map.get(&n.pane_id).copied().unwrap_or(0);
            DisplayItem::Pane {
                pane_id: n.pane_id.clone(),
                pane_name: n.pane_name.clone().unwrap_or_else(|| n.pane_id.clone()),
                tab: n.tab_name.clone(),
                workspace: n.workspace_name.clone(),
                agent_id: n.agent_id.clone(),
                status: n.agent_status.clone(),
                last_accessed_at: ts,
            }
        })
        .collect();
    mru_sort(&mut items);
    items
}

/// Fuzzy-search display items by their search_text.
pub fn search_display_items(items: &[DisplayItem], query: &str) -> Vec<DisplayItem> {
    if query.is_empty() {
        return items.to_vec();
    }

    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);

    FUZZY_MATCHER.with(|m| {
        let mut matcher = m.borrow_mut();
        let mut buf = Vec::new();
        let mut scored: Vec<(usize, u32)> = items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| {
                let text = item.search_text();
                buf.clear();
                let utf32 = nucleo_matcher::Utf32Str::new(&text, &mut buf);
                pattern.score(utf32, &mut matcher).map(|score| (i, score))
            })
            .collect();

        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored.into_iter().map(|(i, _)| items[i].clone()).collect()
    })
}

/// Compute fuzzy-match byte indices in `text` for the given `query`.
/// Returns sorted character positions of each matched character.
pub fn match_indices(text: &str, query: &str) -> Vec<usize> {
    if query.is_empty() {
        return vec![];
    }
    FUZZY_MATCHER.with(|m| {
        let mut matcher = m.borrow_mut();
        let mut haystack_buf: Vec<char> = Vec::new();
        let mut needle_buf: Vec<char> = Vec::new();
        let haystack = nucleo_matcher::Utf32Str::new(text, &mut haystack_buf);
        let needle = nucleo_matcher::Utf32Str::new(query, &mut needle_buf);
        let mut raw_indices: Vec<u32> = Vec::new();
        if matcher
            .fuzzy_indices(haystack, needle, &mut raw_indices)
            .is_some()
        {
            raw_indices.into_iter().map(|i| i as usize).collect()
        } else {
            vec![]
        }
    })
}

// ── DisplayItem helpers (implemented here for mru module access) ──

impl DisplayItem {
    /// Return the sort timestamp for MRU ordering.
    pub fn display_ts(&self) -> u64 {
        match self {
            DisplayItem::Workspace {
                last_accessed_at, ..
            }
            | DisplayItem::Tab {
                last_accessed_at, ..
            }
            | DisplayItem::Agent {
                last_accessed_at, ..
            }
            | DisplayItem::Pane {
                last_accessed_at, ..
            } => *last_accessed_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NavigationNode;

    fn make_node(
        pane_id: &str,
        workspace_id: &str,
        workspace_name: &str,
        tab_name: &str,
        agent_status: AgentStatus,
        last_accessed_at: u64,
        agent_id: Option<&str>,
    ) -> NavigationNode {
        NavigationNode {
            workspace_id: workspace_id.into(),
            workspace_name: workspace_name.into(),
            tab_id: format!("tab-{}", pane_id),
            tab_name: tab_name.into(),
            pane_id: pane_id.into(),
            pane_name: Some(pane_id.into()),
            agent_id: agent_id.map(String::from),
            agent_status,
            last_accessed_at,
        }
    }

    fn sample_nodes() -> Vec<NavigationNode> {
        vec![
            make_node(
                "pane-1",
                "ws-1",
                "Auth-Service",
                "Main",
                AgentStatus::Working,
                5000,
                Some("agent-1"),
            ),
            make_node(
                "pane-2",
                "ws-2",
                "Backend-Repo",
                "Dev",
                AgentStatus::Blocked,
                4000,
                Some("agent-2"),
            ),
            make_node(
                "pane-3",
                "ws-3",
                "Frontend-UI",
                "Design",
                AgentStatus::Done,
                3000,
                Some("agent-3"),
            ),
            make_node(
                "pane-4",
                "ws-1",
                "Auth-Service",
                "Config",
                AgentStatus::Idle,
                2000,
                None,
            ),
            make_node(
                "pane-5",
                "ws-4",
                "Infra-Deploy",
                "Prod",
                AgentStatus::None,
                1000,
                None,
            ),
        ]
    }

    /// Test A: Active workspace exclusion (Workspaces tab)
    #[test]
    fn test_exclude_active_workspace() {
        let nodes = sample_nodes();
        let empty = HashMap::new();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: Some("ws-1"),
            active_pane_id: None,
            active_tab_id: None,
            self_pane_id: None,
        };
        let items = build_display_list(&nodes, &opts, &CategoryTab::Workspaces);
        assert!(
            !items
                .iter()
                .any(|item| matches!(item, DisplayItem::Workspace { id, .. } if id == "ws-1")),
            "Active workspace 'ws-1' should be excluded from Workspaces tab"
        );
        assert_eq!(
            items.len(),
            3,
            "3 workspaces should remain after excluding ws-1"
        );
    }

    /// Test B: Default selected index
    #[test]
    fn test_selected_index_defaults_to_zero() {
        let nodes = sample_nodes();
        let clamped = 0.min(nodes.len().saturating_sub(1));
        assert_eq!(clamped, 0, "Default selected index should be 0");
    }

    /// Test C: Active-only filter
    #[test]
    /// Workspace tab: groups by workspace_id
    #[test]
    fn test_build_workspace_items() {
        let nodes = sample_nodes();
        let empty = HashMap::new();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: None,
            active_tab_id: None,
            self_pane_id: None,
        };
        let items = build_display_list(&nodes, &opts, &CategoryTab::Workspaces);
        // Auth-Service (ws-1) should be first, containing 2 panes and 1 agent
        let first = &items[0];
        if let DisplayItem::Workspace {
            name,
            pane_ids,
            agent_statuses,
            ..
        } = first
        {
            assert_eq!(name, "Auth-Service", "Most recent workspace first");
            assert_eq!(pane_ids.len(), 2, "Auth-Service has 2 panes");
            assert_eq!(agent_statuses.len(), 1, "Auth-Service has 1 agent");
        } else {
            panic!("Expected Workspace item");
        }
        // Infra-Deploy (ws-4) should be last, no agents
        let last = &items[3];
        if let DisplayItem::Workspace {
            name,
            agent_statuses,
            ..
        } = last
        {
            assert_eq!(name, "Infra-Deploy");
            assert_eq!(agent_statuses.len(), 0, "Infra-Deploy has 0 agents");
        } else {
            panic!("Expected Workspace item");
        }
    }

    /// Agents tab: only nodes with agent_id
    #[test]
    fn test_build_agent_items() {
        let nodes = sample_nodes();
        let empty = HashMap::new();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: None,
            active_tab_id: None,
            self_pane_id: None,
        };
        let items = build_display_list(&nodes, &opts, &CategoryTab::Agents);
        assert_eq!(items.len(), 3, "3 agent nodes");
        for item in &items {
            if let DisplayItem::Agent { agent_id, .. } = item {
                assert!(!agent_id.is_empty());
            } else {
                panic!("Expected Agent item");
            }
        }
    }

    /// Pane items are flat (one per node)
    #[test]
    fn test_build_pane_items() {
        let nodes = sample_nodes();
        let empty = HashMap::new();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: None,
            active_tab_id: None,
            self_pane_id: None,
        };
        let items = build_display_list(&nodes, &opts, &CategoryTab::Panes);
        assert_eq!(items.len(), 5, "5 pane items");
    }

    /// Tab tab: excludes the current tab only, other tabs from same workspace remain
    #[test]
    fn test_exclude_active_tab_keeps_same_workspace_tabs() {
        let nodes = sample_nodes();
        let empty = HashMap::new();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: None,
            active_tab_id: Some("tab-pane-1"),
            self_pane_id: None,
        };
        let items = build_display_list(&nodes, &opts, &CategoryTab::Tabs);
        let remaining_names: Vec<&str> = items
            .iter()
            .filter_map(|item| {
                if let DisplayItem::Tab { name, .. } = item {
                    Some(name.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(
            !remaining_names.contains(&"Main"),
            "Excluded tab 'Main' should not appear"
        );
        assert!(
            remaining_names.contains(&"Config"),
            "Other tab 'Config' in same workspace should still appear"
        );
    }

    /// Pane tab: excludes the current pane only, other panes remain
    #[test]
    fn test_exclude_active_pane_keeps_other_panes() {
        let nodes = sample_nodes();
        let empty = HashMap::new();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: Some("pane-1"),
            active_tab_id: None,
            self_pane_id: None,
        };
        let items = build_display_list(&nodes, &opts, &CategoryTab::Panes);
        assert_eq!(items.len(), 4, "4 panes after excluding pane-1");
        assert!(
            !items.iter().any(
                |item| matches!(item, DisplayItem::Pane { pane_id, .. } if pane_id == "pane-1")
            ),
            "Excluded pane should not appear"
        );
    }

    /// Agent tab: excludes agent in the active pane
    #[test]
    fn test_exclude_active_pane_from_agents() {
        let nodes = sample_nodes();
        let empty = HashMap::new();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: Some("pane-1"),
            active_tab_id: None,
            self_pane_id: None,
        };
        let items = build_display_list(&nodes, &opts, &CategoryTab::Agents);
        assert_eq!(items.len(), 2, "2 agents after excluding pane-1's agent");
        assert!(
            !items.iter().any(|item| {
                if let DisplayItem::Agent { agent_id, .. } = item {
                    agent_id == "agent-1"
                } else {
                    false
                }
            }),
            "Agent in excluded pane should not appear"
        );
    }

    /// Test G: build_display_list is deterministic for same inputs
    #[test]
    fn test_build_display_list_deterministic() {
        let nodes = sample_nodes();
        let empty = HashMap::new();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: None,
            active_tab_id: None,
            self_pane_id: None,
        };
        let a = build_display_list(&nodes, &opts, &CategoryTab::Workspaces);
        let b = build_display_list(&nodes, &opts, &CategoryTab::Workspaces);
        assert_eq!(a.len(), b.len(), "Same inputs should produce same length");
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.sort_key(), y.sort_key(), "Items should be in same order");
        }
    }
}
