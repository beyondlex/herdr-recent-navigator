use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};

// AgentStatus is only used in #[cfg(test)] code (make_node helper and test data).
// The import is kept here so `use super::*` in tests can access it.
#[allow(unused_imports)]
use crate::models::{AgentStatus, CategoryTab, DisplayItem, NavigationNode, PaneOthers};

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
    /// Pane runtime state for the All tab (cmd/ssh/cwd).
    pub others: &'a HashMap<String, PaneOthers>,
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
        CategoryTab::All => {
            build_other_items(&all, opts.pane_ts, opts.active_pane_id, opts.self_pane_id, opts.others)
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
        if let Some(pane_ts) = pane_ts.get(&n.pane_id).copied()
            && pane_ts > 0
        {
            merged
                .entry(n.tab_id.clone())
                .and_modify(|e| *e = (*e).max(pane_ts))
                .or_insert(pane_ts);
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

/// Build the "All" display list: one record per (pane × source).
///
/// Excludes agent panes (their identity/buffer is the Agent tab's job) and
/// emits one `DisplayItem::Other` per available source: `ssh` target, `cmd`
/// foreground command (shells suppressed), and `cwd`. Records inherit the
/// pane's MRU timestamp and are ordered by source priority within a pane.
/// The Context column locates the record: connected host (`ssh`), the pane's
/// cwd (`cmd`), or `-` (`cwd` — the detail already is the cwd).
fn build_other_items(
    nodes: &[&NavigationNode],
    ts_map: &HashMap<String, u64>,
    exclude_pane_id: Option<&str>,
    self_pane_id: Option<&str>,
    others: &HashMap<String, PaneOthers>,
) -> Vec<DisplayItem> {
    let mut items: Vec<DisplayItem> = Vec::new();
    for n in nodes {
        if n.agent_id.is_some() {
            continue;
        }
        if !exclude_pane(n, exclude_pane_id, self_pane_id) {
            continue;
        }
        let Some(o) = others.get(&n.pane_id) else {
            continue;
        };
        let ts = ts_map.get(&n.pane_id).copied().unwrap_or(0);
        let pane_name = n
            .pane_name
            .clone()
            .unwrap_or_else(|| n.pane_id.clone());

        let mut push = |source: crate::models::OtherSource, detail: &str, context: String| {
            items.push(DisplayItem::Other {
                pane_id: n.pane_id.clone(),
                pane_name: pane_name.clone(),
                tab: n.tab_name.clone(),
                workspace: n.workspace_name.clone(),
                source,
                detail: detail.to_string(),
                context,
                last_accessed_at: ts,
            });
        };

        if let Some(ssh) = &o.ssh_target {
            // Context shows the connected target with the login user stripped.
            let host = ssh.rsplit('@').next().unwrap_or(ssh);
            push(crate::models::OtherSource::Ssh, ssh, host.to_string());
        }
        if let Some(cmd) = &o.command
            && !crate::others::command_is_shell(cmd)
        {
            let cwd = o.cwd.clone().unwrap_or_else(|| "-".into());
            push(crate::models::OtherSource::Cmd, cmd, cwd);
        }
        if let Some(cwd) = &o.cwd {
            push(crate::models::OtherSource::Cwd, cwd, "-".to_string());
        }
    }
    mru_sort(&mut items);
    items
}

/// Build the Context value for a `file` row: the edited path (made absolute
/// with the pane's cwd when relative) plus `:line` when derivable.
fn file_context(path: String, line: Option<u32>, cwd: Option<&str>) -> String {
    let abs = if path.starts_with('/') {
        path
    } else {
        match cwd {
            Some(c) => format!("{}/{}", c.trim_end_matches('/'), path),
            None => path,
        }
    };
    match line {
        Some(l) => format!("{abs}:{l}"),
        None => abs,
    }
}

/// Build the base list for `.`-prefixed content search in the All tab:
/// one `DisplayItem::Other` per non-agent pane whose buffer matches `query`,
/// with `detail` = a one-line excerpt around the first hit. Rows inherit the
/// pane's MRU timestamp.
///
/// The Type column distinguishes panes that are editing a file (`file`, the
/// excerpt is the buffer's file content) from panes showing plain terminal or
/// command output (`term`), based on the pane's foreground command in `others`
/// (falling back to the pane label when the lazy state hasn't been fetched).
/// A `file` row's Context is the edited path — `:line` when the command
/// carries one — made absolute with the pane's cwd; everything else gets `-`.
/// Classify a content-bearing pane into a `file` (editor buffer) vs `term`
/// (plain terminal output) row, returning whether it is a file row and the
/// edited `(path, line)` when it is.
fn content_row_meta(
    title: &str,
    state: Option<&PaneOthers>,
) -> (bool, Option<(String, Option<u32>)>) {
    let command = state.and_then(|o| o.command.as_deref());
    // The command line that makes this a file row: the foreground command,
    // or the pane label (often the editor cmdline) when process state is
    // not fetched yet.
    let editor_cmd = match command {
        Some(c) if crate::others::command_is_editor(c) => Some(c),
        None if crate::others::command_is_editor(title) => Some(title),
        _ => None,
    };
    let editing_file = !crate::others::is_shell_prompt_title(title) && editor_cmd.is_some();
    let target = if editing_file {
        editor_cmd.and_then(crate::others::parse_editor_target)
    } else {
        None
    };
    (editing_file, target)
}

/// First non-empty line of terminal buffer content, trimmed — the preview a
/// `file `/`term ` row shows when there is no needle to excerpt around.
fn first_content_line(content: &str) -> String {
    content
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .to_string()
}

pub fn build_content_items(
    nodes: &[NavigationNode],
    contents: &HashMap<String, String>,
    others: &HashMap<String, PaneOthers>,
    ts_map: &HashMap<String, u64>,
    exclude_pane_id: Option<&str>,
    self_pane_id: Option<&str>,
    query: &str,
) -> Vec<DisplayItem> {
    let mut items = Vec::new();
    for n in nodes {
        if n.agent_id.is_some() {
            continue;
        }
        if !exclude_pane(&n, exclude_pane_id, self_pane_id) {
            continue;
        }
        let Some(content) = contents.get(&n.pane_id) else {
            continue;
        };
        let Some(detail) = crate::others::content_excerpt(content, query) else {
            continue;
        };
        let title = n.pane_name.as_deref().unwrap_or_default();
        let state = others.get(&n.pane_id);
        let (editing_file, target) = content_row_meta(title, state);
        let context = if editing_file {
            match &target {
                Some((path, line)) => {
                    file_context(path.clone(), *line, state.and_then(|o| o.cwd.as_deref()))
                }
                // Editor with no file argument: fall back to the pane's cwd.
                None => state
                    .and_then(|o| o.cwd.as_deref())
                    .unwrap_or("-")
                    .to_string(),
            }
        } else {
            // `term` rows are located by the pane's cwd.
            state
                .and_then(|o| o.cwd.as_deref())
                .unwrap_or("-")
                .to_string()
        };
        let ts = ts_map.get(&n.pane_id).copied().unwrap_or(0);
        items.push(DisplayItem::Other {
            pane_id: n.pane_id.clone(),
            pane_name: n.pane_name.clone().unwrap_or_else(|| n.pane_id.clone()),
            tab: n.tab_name.clone(),
            workspace: n.workspace_name.clone(),
            source: if editing_file {
                crate::models::OtherSource::File
            } else {
                crate::models::OtherSource::Terminal
            },
            detail,
            context,
            last_accessed_at: ts,
        });
    }
    mru_sort(&mut items);
    items
}

/// List every non-agent pane of the given content source kind (`File` = an
/// editor buffer, `Terminal` = plain terminal output) that has cached buffer
/// content, regardless of needle. Backs `file ` / `term ` with an empty
/// needle: the Detail column previews the edited path (`file`) or the first
/// content line (`term`), since there is no match to excerpt around.
fn build_content_source_items(
    nodes: &[NavigationNode],
    contents: &HashMap<String, String>,
    others: &HashMap<String, PaneOthers>,
    ts_map: &HashMap<String, u64>,
    exclude_pane_id: Option<&str>,
    self_pane_id: Option<&str>,
    src: crate::models::OtherSource,
) -> Vec<DisplayItem> {
    let mut items = Vec::new();
    for n in nodes {
        if n.agent_id.is_some() {
            continue;
        }
        if !exclude_pane(&n, exclude_pane_id, self_pane_id) {
            continue;
        }
        let Some(content) = contents.get(&n.pane_id) else {
            continue;
        };
        let title = n.pane_name.as_deref().unwrap_or_default();
        let state = others.get(&n.pane_id);
        let (editing_file, target) = content_row_meta(title, state);
        let matches_src = if src == crate::models::OtherSource::File {
            editing_file
        } else {
            !editing_file
        };
        if !matches_src {
            continue;
        }
        let cwd = state.and_then(|o| o.cwd.as_deref());
        let detail = match &target {
            Some((path, _)) => file_context(path.clone(), None, cwd),
            None => first_content_line(content),
        };
        let context = if editing_file {
            match &target {
                Some((path, line)) => file_context(path.clone(), *line, cwd),
                // Editor with no file argument: fall back to the pane's cwd.
                None => cwd.unwrap_or("-").to_string(),
            }
        } else {
            // `term` rows are located by the pane's cwd.
            cwd.unwrap_or("-").to_string()
        };
        let ts = ts_map.get(&n.pane_id).copied().unwrap_or(0);
        items.push(DisplayItem::Other {
            pane_id: n.pane_id.clone(),
            pane_name: n.pane_name.clone().unwrap_or_else(|| n.pane_id.clone()),
            tab: n.tab_name.clone(),
            workspace: n.workspace_name.clone(),
            source: if editing_file {
                crate::models::OtherSource::File
            } else {
                crate::models::OtherSource::Terminal
            },
            detail,
            context,
            last_accessed_at: ts,
        });
    }
    mru_sort(&mut items);
    items
}

/// Base list for the All tab: pane-state records (cmd/ssh/cwd) and
/// pane-buffer content matches in ONE list. A plain query searches both —
/// buffers by substring, state rows fuzzy. The query may carry a narrowing
/// filter prefix (`.` = buffer content only, `cmd `/`ssh `/`cwd `/`file `/
/// `term ` = that source only, `ws `/`tab `/`pane ` = the workspace, tab or
/// pane list).
///
/// Returns the query to fuzzy-rank and highlight with, plus the unranked
/// rows. With an empty needle, filtered rows still list: state rows show
/// every record of that source (so `ssh ` alone shows every ssh pane),
/// `file `/`term ` show every buffer pane of that kind with a preview, and
/// `ws `/`tab `/`pane ` show every workspace/tab/pane (MRU). The bare `.`
/// content filter needs a needle (an excerpt needs a match), so it stays
/// empty.
pub fn build_others_base(
    nodes: &[NavigationNode],
    contents: &HashMap<String, String>,
    others: &HashMap<String, PaneOthers>,
    opts: &BuildOptions,
    query: &str,
) -> (String, Vec<DisplayItem>) {
    let (filter, rest) = crate::models::OthersFilter::parse(query);
    let needle = rest.trim();
    let content_rows = |n: &str| {
        if n.is_empty() {
            Vec::new()
        } else {
            build_content_items(
                nodes,
                contents,
                others,
                opts.pane_ts,
                opts.active_pane_id,
                opts.self_pane_id,
                n,
            )
        }
    };
    let by_source =
        |items: Vec<DisplayItem>, src: crate::models::OtherSource| -> Vec<DisplayItem> {
            items
                .into_iter()
                .filter(|it| matches!(it, DisplayItem::Other { source, .. } if *source == src))
                .collect()
        };

    let items = match filter {
        Some(crate::models::OthersFilter::Source(src)) => match src {
            // `file ` / `term ` with no needle: list the panes of that kind
            // (an excerpt needs a match; fall back to a path/first-line preview).
            crate::models::OtherSource::File | crate::models::OtherSource::Terminal
                if needle.is_empty() =>
            {
                build_content_source_items(
                    nodes,
                    contents,
                    others,
                    opts.pane_ts,
                    opts.active_pane_id,
                    opts.self_pane_id,
                    src,
                )
            }
            crate::models::OtherSource::File | crate::models::OtherSource::Terminal => {
                by_source(content_rows(needle), src)
            }
            _ => by_source(build_display_list(nodes, opts, &CategoryTab::All), src),
        },
        // `ws ` / `tab ` / `pane ` swap the list to the matching dimension's
        // entities, so typing a name fuzzy-filters down to it directly.
        Some(crate::models::OthersFilter::Workspace) => {
            build_display_list(nodes, opts, &CategoryTab::Workspaces)
        }
        Some(crate::models::OthersFilter::Tab) => {
            build_display_list(nodes, opts, &CategoryTab::Tabs)
        }
        Some(crate::models::OthersFilter::Pane) => {
            build_display_list(nodes, opts, &CategoryTab::Panes)
        }
        Some(crate::models::OthersFilter::Content) => content_rows(needle),
        None => {
            let mut items = build_display_list(nodes, opts, &CategoryTab::All);
            if !needle.is_empty() {
                items.extend(content_rows(needle));
            }
            items
        }
    };
    let ranked_query = if filter.is_some() {
        needle.to_string()
    } else {
        query.to_string()
    };
    (ranked_query, items)
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
        // (primary, secondary) sort keys. For `Other` rows the primary key is
        // the score against the detail column alone: a match in the pane/tab/
        // workspace columns must never outrank a detail column match.
        let mut scored: Vec<((u32, u32), usize)> = items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| {
                let text = item.search_text();
                buf.clear();
                let utf32 = nucleo_matcher::Utf32Str::new(&text, &mut buf);
                let full = pattern.score(utf32, &mut matcher)?;
                let (pri, sec) = match item {
                    DisplayItem::Other { detail, .. } => {
                        let mut detail_buf = Vec::new();
                        let detail32 = nucleo_matcher::Utf32Str::new(detail, &mut detail_buf);
                        (pattern.score(detail32, &mut matcher).unwrap_or(0), full)
                    }
                    _ => (full, 0),
                };
                Some(((pri, sec), i))
            })
            .collect();

        scored.sort_by_key(|(k, _)| Reverse(*k));
        scored.into_iter().map(|(_, i)| items[i].clone()).collect()
    })
}

/// Compute fuzzy-match character indices in `text` for the given `query`,
/// using the same case-insensitive `Pattern` matcher as row ranking so a
/// highlighted row always shows its hits regardless of case.
///
/// Returns sorted, deduplicated character positions of matched characters.
pub fn match_indices(text: &str, query: &str) -> Vec<usize> {
    if query.is_empty() {
        return vec![];
    }
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    FUZZY_MATCHER.with(|m| {
        let mut matcher = m.borrow_mut();
        let mut haystack_buf: Vec<char> = Vec::new();
        let haystack = nucleo_matcher::Utf32Str::new(text, &mut haystack_buf);
        let mut raw_indices: Vec<u32> = Vec::new();
        if pattern.indices(haystack, &mut matcher, &mut raw_indices).is_some() {
            raw_indices.sort_unstable();
            raw_indices.dedup();
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
            }
            | DisplayItem::Other {
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
        let empty_others = HashMap::new();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: Some("ws-1"),
            active_pane_id: None,
            active_tab_id: None,
            self_pane_id: None,
            others: &empty_others,
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
        let clamped = 0;
        assert_eq!(clamped, 0, "Default selected index should be 0");
    }

    /// Test C: Workspace tab: groups by workspace_id
    #[test]
    fn test_build_workspace_items() {
        let nodes = sample_nodes();
        let empty = HashMap::new();
        let empty_others = HashMap::new();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: None,
            active_tab_id: None,
            self_pane_id: None,
            others: &empty_others,
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
        let empty_others = HashMap::new();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: None,
            active_tab_id: None,
            self_pane_id: None,
            others: &empty_others,
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
        let empty_others = HashMap::new();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: None,
            active_tab_id: None,
            self_pane_id: None,
            others: &empty_others,
        };
        let items = build_display_list(&nodes, &opts, &CategoryTab::Panes);
        assert_eq!(items.len(), 5, "5 pane items");
    }

    /// Tab tab: excludes the current tab only, other tabs from same workspace remain
    #[test]
    fn test_exclude_active_tab_keeps_same_workspace_tabs() {
        let nodes = sample_nodes();
        let empty = HashMap::new();
        let empty_others = HashMap::new();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: None,
            active_tab_id: Some("tab-pane-1"),
            self_pane_id: None,
            others: &empty_others,
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
        let empty_others = HashMap::new();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: Some("pane-1"),
            active_tab_id: None,
            self_pane_id: None,
            others: &empty_others,
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
        let empty_others = HashMap::new();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: Some("pane-1"),
            active_tab_id: None,
            self_pane_id: None,
            others: &empty_others,
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
        let empty_others = HashMap::new();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: None,
            active_tab_id: None,
            self_pane_id: None,
            others: &empty_others,
        };
        let a = build_display_list(&nodes, &opts, &CategoryTab::Workspaces);
        let b = build_display_list(&nodes, &opts, &CategoryTab::Workspaces);
        assert_eq!(a.len(), b.len(), "Same inputs should produce same length");
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.sort_key(), y.sort_key(), "Items should be in same order");
        }
    }

    // ── All tab ──

    fn others_map() -> HashMap<String, PaneOthers> {
        let mut map: HashMap<String, PaneOthers> = HashMap::new();
        map.insert(
            "pane-4".into(),
            PaneOthers {
                cwd: Some("/repo/auth/config".into()),
                command: Some("ssh -p 2222 deploy@10.1.2.3".into()),
                ssh_target: Some("deploy@10.1.2.3:2222".into()),
            },
        );
        map.insert(
            "pane-5".into(),
            PaneOthers {
                cwd: Some("/infra/prod".into()),
                command: Some("-zsh".into()),
                ssh_target: None,
            },
        );
        map
    }

    /// All emits one record per (pane × source); agent panes and login
    /// shells are excluded; records group by pane in priority order.
    #[test]
    fn test_build_other_items_records_and_excludes() {
        let nodes = sample_nodes();
        let empty = HashMap::new();
        let others = others_map();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: None,
            active_tab_id: None,
            self_pane_id: None,
            others: &others,
        };
        let items = build_display_list(&nodes, &opts, &CategoryTab::All);
        // pane-4: ssh + cmd + cwd = 3 records; pane-5: cwd only (shell filtered) = 1.
        assert_eq!(items.len(), 4);
        for item in &items {
            if let DisplayItem::Other { pane_id, .. } = item {
                assert!(
                    !matches!(pane_id.as_str(), "pane-1" | "pane-2" | "pane-3"),
                    "agent panes must be excluded from All"
                );
            } else {
                panic!("Expected Other item");
            }
        }
        // Records of pane-4 are grouped and ordered ssh < cmd < cwd.
        let pane4_sources: Vec<crate::models::OtherSource> = items
            .iter()
            .filter_map(|it| match it {
                DisplayItem::Other { pane_id, source, .. } if pane_id == "pane-4" => Some(*source),
                _ => None,
            })
            .collect();
        assert_eq!(
            pane4_sources,
            vec![
                crate::models::OtherSource::Ssh,
                crate::models::OtherSource::Cmd,
                crate::models::OtherSource::Cwd,
            ]
        );
    }

    /// Context locates each record: connected host (user stripped) for ssh,
    /// the pane cwd for cmd, `-` for cwd (detail already is the cwd).
    #[test]
    fn test_build_other_items_context_column() {
        let nodes = sample_nodes();
        let empty = HashMap::new();
        let others = others_map();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: None,
            active_tab_id: None,
            self_pane_id: None,
            others: &others,
        };
        let items = build_display_list(&nodes, &opts, &CategoryTab::All);
        let ctx_of = |pane: &str, src: crate::models::OtherSource| -> String {
            items
                .iter()
                .filter_map(|it| match it {
                    DisplayItem::Other {
                        pane_id,
                        source,
                        context,
                        ..
                    } if pane_id == pane && *source == src => Some(context.clone()),
                    _ => None,
                })
                .next()
                .unwrap_or_default()
        };
        assert_eq!(
            ctx_of("pane-4", crate::models::OtherSource::Ssh),
            "10.1.2.3:2222"
        );
        assert_eq!(
            ctx_of("pane-4", crate::models::OtherSource::Cmd),
            "/repo/auth/config"
        );
        assert_eq!(ctx_of("pane-5", crate::models::OtherSource::Cwd), "-");
    }

    /// Content search emits File rows only for matching panes; agent panes and
    /// the excluded/self panes never appear.
    #[test]
    fn test_build_content_items_matches_and_excludes() {
        let nodes = sample_nodes();
        let empty = HashMap::new();
        let mut contents = HashMap::new();
        contents.insert("pane-4".to_string(), "Started DeployService on 8081\nnext line\n".to_string());
        contents.insert("pane-3".to_string(), "hot text here\n".to_string()); // agent pane
        // pane-4 is editing a file, pane-5 shows terminal output.
        let mut others: HashMap<String, PaneOthers> = HashMap::new();
        others.insert(
            "pane-4".into(),
            PaneOthers { cwd: None, command: Some("nvim src/main.rs".into()), ssh_target: None },
        );
        others.insert(
            "pane-5".into(),
            PaneOthers { cwd: None, command: Some("-zsh".into()), ssh_target: None },
        );

        let items = build_content_items(&nodes, &contents, &others, &empty, None, Some("pane-5"), "deployservice");
        assert_eq!(items.len(), 1);
        let item = &items[0];
        match item {
            DisplayItem::Other { pane_id, source, detail, context, .. } => {
                assert_eq!(pane_id, "pane-4");
                assert_eq!(*source, crate::models::OtherSource::File);
                assert!(detail.eq_ignore_ascii_case("started deployservice on 8081"));
                assert_eq!(
                    context, "src/main.rs",
                    "file context = edited path (no cwd known)"
                );
            }
            _ => panic!("expected Other item"),
        }

        // A terminal-output pane is typed `term`, not `file`.
        contents.insert("pane-5".to_string(), "deploy failed: connection refused\n".to_string());
        let items = build_content_items(&nodes, &contents, &others, &empty, None, None, "deploy");
        let pane5 = items
            .iter()
            .find(|it| matches!(it, DisplayItem::Other { pane_id, .. } if pane_id == "pane-5"))
            .expect("pane-5 matched");
        assert_eq!(
            match pane5 {
                DisplayItem::Other { source, .. } => *source,
                _ => unreachable!(),
            },
            crate::models::OtherSource::Terminal
        );

        // No match → empty list; agent/content never searched.
        let items = build_content_items(&nodes, &contents, &others, &empty, None, Some("pane-5"), "zzz-no");
        assert!(items.is_empty());

        // No cached content for any pane → nothing to match.
        let items = build_content_items(&nodes, &HashMap::new(), &others, &empty, None, Some("pane-5"), "deployservice");
        assert!(items.is_empty());
    }

    /// A `file` row's Context is the edited path with `:line` when the
    /// command carries one; an editor without a file argument degrades to `-`.
    #[test]
    fn test_build_content_items_file_context() {
        let nodes = sample_nodes();
        let empty = HashMap::new();
        let mut contents = HashMap::new();
        contents.insert("pane-4".to_string(), "deploy the service\n".to_string());
        let mut others: HashMap<String, PaneOthers> = HashMap::new();
        others.insert(
            "pane-4".into(),
            PaneOthers {
                cwd: Some("/repo/auth".into()),
                command: Some("nvim +328 src/main.rs".into()),
                ssh_target: None,
            },
        );
        let items = build_content_items(&nodes, &contents, &others, &empty, None, None, "deploy");
        assert_eq!(items.len(), 1);
        match &items[0] {
            DisplayItem::Other {
                source, context, ..
            } => {
                assert_eq!(*source, crate::models::OtherSource::File);
                assert_eq!(context, "/repo/auth/src/main.rs:328");
            }
            _ => panic!("expected Other item"),
        }

        // Editor with no file argument: still a `file` row, Context is `-`.
        others.insert(
            "pane-5".into(),
            PaneOthers {
                cwd: None,
                command: Some("hx".into()),
                ssh_target: None,
            },
        );
        contents.insert("pane-5".to_string(), "term output deploy\n".to_string());
        let items = build_content_items(&nodes, &contents, &others, &empty, None, None, "deploy");
        let pane5 = items
            .iter()
            .find(|it| matches!(it, DisplayItem::Other { pane_id, .. } if pane_id == "pane-5"))
            .expect("pane-5 matched");
        assert_eq!(
            match pane5 {
                DisplayItem::Other { context, .. } => context.as_str(),
                _ => unreachable!(),
            },
            "-"
        );
    }

    /// A shell-prompt-shaped pane title is a reverse signal: the pane is a
    /// terminal even when the foreground command names an editor binary.
    #[test]
    fn test_shell_prompt_title_overrides_editor_command() {
        let mut nodes = sample_nodes();
        nodes.push(NavigationNode {
            workspace_id: "ws-5".into(),
            workspace_name: "Ops".into(),
            tab_id: "tab-pane-6".into(),
            tab_name: "Main".into(),
            pane_id: "pane-6".into(),
            pane_name: Some("user@192.168.4.1:~/ops/logs".into()),
            agent_id: None,
            agent_status: AgentStatus::None,
            last_accessed_at: 900,
        });
        let empty = HashMap::new();
        let mut contents = HashMap::new();
        contents.insert("pane-6".to_string(), "ERROR: auth token expired\n".to_string());
        let mut others = HashMap::new();
        others.insert(
            "pane-6".into(),
            PaneOthers { cwd: None, command: Some("vim dump.log".into()), ssh_target: None },
        );
        let items = build_content_items(&nodes, &contents, &others, &empty, None, None, "token");
        assert_eq!(items.len(), 1);
        assert_eq!(
            match &items[0] {
                DisplayItem::Other { source, .. } => *source,
                _ => unreachable!(),
            },
            crate::models::OtherSource::Terminal
        );
    }

    /// All base list: a plain query mixes state records and buffer
    /// content matches; a leading `.` narrows to buffer content only; an
    /// empty query lists state records alone.
    #[test]
    fn test_build_others_base_mixes_state_and_content() {
        let nodes = sample_nodes();
        let empty = HashMap::new();
        let mut contents = HashMap::new();
        contents.insert(
            "pane-4".to_string(),
            "Started DeployService on 8081\n".to_string(),
        );
        let others = others_map(); // pane-4: ssh + cmd records; pane-5: cwd
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: None,
            active_tab_id: None,
            self_pane_id: None,
            others: &others,
        };
        let is_source = |items: &[DisplayItem], src: crate::models::OtherSource| {
            items
                .iter()
                .any(|it| matches!(it, DisplayItem::Other { source, .. } if *source == src))
        };

        // Plain query: state records AND the buffer match are both present.
        let (q, items) = build_others_base(&nodes, &contents, &others, &opts, "deploy");
        assert_eq!(q, "deploy");
        assert!(
            is_source(&items, crate::models::OtherSource::Ssh),
            "state records included"
        );
        assert!(
            is_source(&items, crate::models::OtherSource::Terminal),
            "buffer match included"
        );

        // Leading `.`: buffer content only.
        let (q, items) = build_others_base(&nodes, &contents, &others, &opts, ".deploy");
        assert_eq!(q, "deploy");
        assert!(is_source(&items, crate::models::OtherSource::Terminal));
        assert!(
            !is_source(&items, crate::models::OtherSource::Ssh),
            "state records excluded in `.` mode"
        );

        // Empty query: state records alone (no needle, no excerpts).
        let (q, items) = build_others_base(&nodes, &contents, &others, &opts, "");
        assert_eq!(q, "");
        assert_eq!(items.len(), 4, "ssh+cmd+cwd of pane-4, cwd of pane-5");

        // Bare `.`: empty needle, nothing to show.
        let (q, items) = build_others_base(&nodes, &contents, &others, &opts, ".");
        assert_eq!(q, "");
        assert!(items.is_empty());
    }

    /// Source filters narrow to one record type: `cmd ` alone lists every cmd
    /// row; `file ` filters content rows down to file buffers.
    #[test]
    fn test_build_others_base_source_filters() {
        let nodes = sample_nodes();
        let empty = HashMap::new();
        let mut contents = HashMap::new();
        contents.insert(
            "pane-4".to_string(),
            "Started DeployService on 8081\n".to_string(),
        );
        // pane-4 edits a file (nvim), so its buffer match types as `file`.
        let mut others = others_map();
        others.insert(
            "pane-4".into(),
            PaneOthers {
                cwd: Some("/repo/auth".into()),
                command: Some("nvim src/main.rs".into()),
                ssh_target: Some("deploy@10.1.2.3:2222".into()),
            },
        );
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: None,
            active_tab_id: None,
            self_pane_id: None,
            others: &others,
        };
        let sources = |items: &[DisplayItem]| -> Vec<crate::models::OtherSource> {
            items
                .iter()
                .filter_map(|it| match it {
                    DisplayItem::Other { source, .. } => Some(*source),
                    _ => None,
                })
                .collect()
        };

        // `cmd ` with no needle: every cmd row, nothing else.
        let (q, items) = build_others_base(&nodes, &contents, &others, &opts, "cmd ");
        assert_eq!(q, "");
        assert_eq!(sources(&items), vec![crate::models::OtherSource::Cmd]);

        // `ssh deploy`: ssh rows only, ranked by the remaining text.
        let (q, items) = build_others_base(&nodes, &contents, &others, &opts, "ssh deploy");
        assert_eq!(q, "deploy");
        assert_eq!(sources(&items), vec![crate::models::OtherSource::Ssh]);

        // `file deploy`: content match typed `file`, no state rows.
        let (q, items) = build_others_base(&nodes, &contents, &others, &opts, "file deploy");
        assert_eq!(q, "deploy");
        assert_eq!(sources(&items), vec![crate::models::OtherSource::File]);

        // `file ` with no needle: every file-editing pane lists, detail falls
        // back to the edited path (no match to excerpt around).
        let (q, items) = build_others_base(&nodes, &contents, &others, &opts, "file ");
        assert_eq!(q, "");
        assert_eq!(sources(&items), vec![crate::models::OtherSource::File]);
        match &items[0] {
            DisplayItem::Other {
                pane_id, detail, context, ..
            } => {
                assert_eq!(pane_id, "pane-4");
                assert_eq!(detail, "/repo/auth/src/main.rs");
                assert_eq!(context, "/repo/auth/src/main.rs");
            }
            _ => panic!("expected Other item"),
        }

        // `term ` with no needle: terminal-output panes list, detail falls
        // back to the first content line and context to the pane's cwd.
        contents.insert(
            "pane-5".to_string(),
            "  \nls: No such file or directory\n".to_string(),
        );
        let (q, items) = build_others_base(&nodes, &contents, &others, &opts, "term ");
        assert_eq!(q, "");
        assert_eq!(sources(&items), vec![crate::models::OtherSource::Terminal]);
        match &items[0] {
            DisplayItem::Other {
                pane_id, detail, context, ..
            } => {
                assert_eq!(pane_id, "pane-5");
                assert_eq!(detail, "ls: No such file or directory");
                assert_eq!(context, "/infra/prod");
            }
            _ => panic!("expected Other item"),
        }
    }

    /// Dimension filters swap the All list to that dimension's entities:
    /// `ws ` lists workspaces, `tab ` lists tabs; typing a name fuzzy-filters
    /// the list down to matching entities.
    #[test]
    fn test_build_others_base_ws_tab_filters() {
        let nodes = sample_nodes();
        let empty = HashMap::new();
        let contents = HashMap::new();
        let others = others_map();
        let opts = BuildOptions {
            pane_ts: &empty,
            tab_ts: &empty,
            ws_ts: &empty,
            active_workspace_id: None,
            active_pane_id: None,
            active_tab_id: None,
            self_pane_id: None,
            others: &others,
        };

        // `ws ` (empty needle): the workspace list.
        let (q, items) = build_others_base(&nodes, &contents, &others, &opts, "ws ");
        assert_eq!(q, "");
        let names: Vec<&str> = items
            .iter()
            .filter_map(|it| match it {
                DisplayItem::Workspace { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            names,
            vec![
                "Auth-Service",
                "Backend-Repo",
                "Frontend-UI",
                "Infra-Deploy"
            ]
        );

        // `tab ` (empty needle): the tab list.
        let (q, items) = build_others_base(&nodes, &contents, &others, &opts, "tab ");
        assert_eq!(q, "");
        assert!(!items.is_empty());
        assert!(items.iter().all(|it| matches!(it, DisplayItem::Tab { .. })));

        // Fuzzy-filter the two lists with a needle (ranked_query is consumed
        // by search_display_items, exactly as in the app event loop).
        let (q, items) = build_others_base(&nodes, &contents, &others, &opts, "ws auth");
        assert_eq!(q, "auth");
        let ranked = search_display_items(&items, &q);
        let names: Vec<&str> = ranked
            .iter()
            .filter_map(|it| match it {
                DisplayItem::Workspace { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["Auth-Service"]);

        let (q, items) = build_others_base(&nodes, &contents, &others, &opts, "tab prod");
        assert_eq!(q, "prod");
        let ranked = search_display_items(&items, &q);
        let names: Vec<&str> = ranked
            .iter()
            .filter_map(|it| match it {
                DisplayItem::Tab { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["Prod"]);

        let (q, items) = build_others_base(&nodes, &contents, &others, &opts, "pane pane-4");
        assert_eq!(q, "pane-4");
        let ranked = search_display_items(&items, &q);
        let names: Vec<&str> = ranked
            .iter()
            .filter_map(|it| match it {
                DisplayItem::Pane { pane_name, .. } => Some(pane_name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["pane-4"]);

        // No workspace/tab/pane matches the text: empty result.
        let (q, items) = build_others_base(&nodes, &contents, &others, &opts, "ws nope");
        assert!(search_display_items(&items, &q).is_empty());
        let (q, items) = build_others_base(&nodes, &contents, &others, &opts, "tab nope");
        assert!(search_display_items(&items, &q).is_empty());
        let (q, items) = build_others_base(&nodes, &contents, &others, &opts, "pane nope");
        assert!(search_display_items(&items, &q).is_empty());
    }

    /// All ranking must favor the detail column: a workspace/tab/pane-only
    /// fuzzy hit must not outrank a row whose detail fully matches the query.
    #[test]
    fn test_other_search_prioritizes_detail_column() {
        let items: Vec<DisplayItem> = vec![
            DisplayItem::Other {
                pane_id: "p-detail".into(),
                pane_name: "p-detail".into(),
                tab: "main".into(),
                workspace: "main".into(),
                source: crate::models::OtherSource::Ssh,
                detail: "ssh -p 2222 deploy@app-server-01".into(),
                context: "-".into(),
                last_accessed_at: 2000,
            },
            DisplayItem::Other {
                pane_id: "p-workspace".into(),
                pane_name: "p-workspace".into(),
                tab: "main".into(),
                workspace: "app-server-01-prod".into(),
                source: crate::models::OtherSource::Cmd,
                detail: "tail -f /var/log/app.log".into(),
                context: "-".into(),
                last_accessed_at: 1000,
            },
        ];
        // Sanity: the workspace-column row scores higher on the full search
        // text (nucleo sprinkles bonus characters across fields), so the
        // regression is precisely that detail still wins.
        let pattern = Pattern::parse("app-server-01", CaseMatching::Ignore, Normalization::Smart);
        FUZZY_MATCHER.with(|m| {
            let mut matcher = m.borrow_mut();
            let mut buf = Vec::new();
            let full = pattern
                .score(
                    nucleo_matcher::Utf32Str::new(&items[1].search_text(), &mut buf),
                    &mut matcher,
                )
                .unwrap();
            let mut dbuf = Vec::new();
            let detail_text = match &items[0] {
                DisplayItem::Other { detail, .. } => detail.as_str(),
                _ => unreachable!(),
            };
            let detail = pattern
                .score(
                    nucleo_matcher::Utf32Str::new(detail_text, &mut dbuf),
                    &mut matcher,
                )
                .unwrap();
            assert!(full > detail, "precondition: workspace full-text hit outranks detail");
        });

        let ranked = search_display_items(&items, "app-server-01");
        assert_eq!(ranked.len(), 2);
        let first = match &ranked[0] {
            DisplayItem::Other { detail, .. } => detail.as_str(),
            _ => unreachable!(),
        };
        assert!(
            first.contains("deploy@"),
            "detail column match must rank first, got: {first}"
        );
    }

    /// All search text includes the source label so `ssh` filters to ssh rows.
    #[test]
    fn test_other_search_text_includes_source_label() {
        let others = others_map();
        let node = make_node(
            "pane-4",
            "ws-1",
            "Auth-Service",
            "Config",
            AgentStatus::Idle,
            2000,
            None,
        );
        let records = build_other_items(
            &[&node],
            &HashMap::new(),
            None,
            None,
            &others,
        );
        let ssh_record = records
            .iter()
            .find(|r| matches!(r, DisplayItem::Other { source: crate::models::OtherSource::Ssh, .. }))
            .expect("ssh record exists");
        let text = ssh_record.search_text();
        assert!(text.starts_with("ssh "), "source label first: {text}");
        assert!(text.contains("deploy@10.1.2.3:2222"));
    }

    /// Regression: highlight must not panic on the content that crashed the
    /// TUI with `.Rep` (nucleo case-sensitive `fuzzy_indices` assert fired on
    /// this haystack; `Pattern` + Ignore, which ranks rows, is safe).
    #[test]
    fn test_match_indices_no_panic_on_regression_text() {
        // The exact hit that reproduced the crash: needle `Rep` vs the detail
        // excerpt from an nvim pane buffer.
        let text = "ERROR: Repository not found.";
        let idx = match_indices(text, "Rep");
        assert!(!idx.is_empty(), "Rep is present in {text:?}");
        let needles = ["R", "Re", "Rep", "th", "thers", "Others"];
        for q in needles {
            let _ = match_indices(text, q);
        }
    }

    /// Matches should be case-insensitive, consistent with row ranking
    /// (`Pattern::parse(.., CaseMatching::Ignore, ..)`).
    #[test]
    fn test_match_indices_is_case_insensitive() {
        assert_eq!(
            match_indices("Permission denied", "permission"),
            vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
        );
        assert_eq!(
            match_indices("Permission denied", "PERMISSION"),
            vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
        );
        assert_eq!(match_indices("Others", "others"), vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(match_indices("others", "Others"), vec![0, 1, 2, 3, 4, 5]);
    }
}
