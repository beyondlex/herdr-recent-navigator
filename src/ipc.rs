use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::models::{AgentStatus, NavigationNode, PaneOthers, WORKTREE_SEP};

/// Information about the currently focused pane, captured during
/// `fetch_all_nodes()` to avoid a redundant subprocess call.
#[derive(Debug, Clone)]
pub struct FocusedPaneInfo {
    pub pane_id: String,
    pub tab_id: String,
    pub workspace_id: String,
    pub label: String,
}

// ── Herdr CLI response types (minimal subset) ──

/// Wrapper for herdr CLI JSON responses: {"id":"...","result":{...}}
#[derive(Debug, Deserialize)]
struct CliResponse<R> {
    result: R,
}

#[derive(Debug, Deserialize)]
struct WorkspaceListResult {
    workspaces: Vec<WorkspaceInfo>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceInfo {
    workspace_id: String,
    label: String,
    #[serde(default)]
    worktree: Option<WorktreeInfo>,
}

#[derive(Debug, Deserialize)]
struct WorktreeInfo {
    is_linked_worktree: bool,
    repo_key: String,
    repo_name: String,
}

/// Workspace label as shown in the navigator: a linked git worktree is
/// prefixed with its main-checkout workspace label (or repo name when that
/// workspace is absent) so it groups under, and searches with, its repo.
fn workspace_labels(workspaces: &[WorkspaceInfo]) -> HashMap<String, String> {
    let main_labels: HashMap<&str, &str> = workspaces
        .iter()
        .filter_map(|w| match &w.worktree {
            Some(t) if !t.is_linked_worktree => Some((t.repo_key.as_str(), w.label.as_str())),
            _ => None,
        })
        .collect();
    workspaces
        .iter()
        .map(|w| {
            let label = match &w.worktree {
                Some(t) if t.is_linked_worktree => {
                    let repo = main_labels
                        .get(t.repo_key.as_str())
                        .copied()
                        .unwrap_or(&t.repo_name);
                    format!("{repo}{WORKTREE_SEP}{}", w.label)
                }
                _ => w.label.clone(),
            };
            (w.workspace_id.clone(), label)
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct PaneListResult {
    panes: Vec<PaneInfo>,
}

#[derive(Debug, Deserialize)]
struct PaneInfo {
    pane_id: String,
    workspace_id: String,
    tab_id: String,
    focused: bool,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    agent_status: Option<AgentStatusWire>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    foreground_cwd: Option<String>,
}

/// Wrapper around `herdr pane process-info` result field.
#[derive(Debug, Deserialize)]
struct ProcessInfoResult {
    #[serde(default)]
    process_info: Option<ProcessInfo>,
}

/// One pane's process tree info from `herdr pane process-info`.
#[derive(Debug, Deserialize)]
struct ProcessInfo {
    #[serde(default)]
    foreground_processes: Vec<ForegroundProcess>,
}

/// A single foreground process entry within a pane.
#[derive(Debug, Deserialize)]
struct ForegroundProcess {
    #[serde(default)]
    argv: Option<Vec<String>>,
    #[serde(default)]
    cmdline: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TabListResult {
    tabs: Vec<TabInfo>,
}

#[derive(Debug, Deserialize)]
struct TabInfo {
    tab_id: String,
    #[serde(default)]
    workspace_id: String,
    #[serde(default)]
    label: Option<String>,
}

/// Wire-level agent status as returned by herdr.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
enum AgentStatusWire {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
}

impl From<AgentStatusWire> for AgentStatus {
    fn from(w: AgentStatusWire) -> Self {
        match w {
            AgentStatusWire::Working => AgentStatus::Working,
            AgentStatusWire::Blocked => AgentStatus::Blocked,
            AgentStatusWire::Done => AgentStatus::Done,
            AgentStatusWire::Idle => AgentStatus::Idle,
            AgentStatusWire::Unknown => AgentStatus::None,
        }
    }
}

// ── Test mock infrastructure ──

#[cfg(test)]
mod mock_io {
    use once_cell::sync::Lazy;
    use std::process::Output;
    use std::sync::Mutex;

    static MOCK_OUTPUTS: Lazy<Mutex<Vec<Output>>> = Lazy::new(|| Mutex::new(Vec::new()));

    pub fn make_output(stdout: &str) -> Output {
        use std::os::unix::process::ExitStatusExt;
        Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    pub fn make_failing_output() -> Output {
        use std::os::unix::process::ExitStatusExt;
        Output {
            status: std::process::ExitStatus::from_raw(1),
            stdout: b"{}".to_vec(),
            stderr: b"error".to_vec(),
        }
    }

    /// Set mock outputs for the next herdr_cli calls.
    /// Each call consumes one output from the front of the vec.
    pub fn set_mock_outputs(outputs: Vec<Output>) {
        let mut store = MOCK_OUTPUTS.lock().unwrap();
        *store = outputs.into_iter().rev().collect();
    }

    /// Clear all mock outputs (call before each test to avoid cross-test pollution).
    pub fn clear() {
        let mut store = MOCK_OUTPUTS.lock().unwrap();
        store.clear();
    }

    pub fn pop_mock_output() -> Option<Output> {
        let mut store = MOCK_OUTPUTS.lock().unwrap();
        store.pop()
    }
}

// ── HerdrClient ──

/// Resolved herdr binary path (from HERDR_BIN_PATH env, or fallback to "herdr").
fn herdr_bin() -> String {
    std::env::var("HERDR_BIN_PATH")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "herdr".to_string())
}

/// Path to the Herdr UDS socket (from HERDR_SOCKET_PATH).
fn sock_path() -> Option<String> {
    std::env::var("HERDR_SOCKET_PATH")
        .ok()
        .filter(|s| !s.is_empty())
}

/// Map CLI-style args to JSON-RPC method name and params.
fn args_to_method(args: &[&str]) -> Option<(&'static str, serde_json::Value)> {
    use serde_json::json;
    match args {
        ["workspace", "list"] => Some(("workspace.list", json!({}))),
        ["tab", "list"] => Some(("tab.list", json!({}))),
        ["pane", "list"] => Some(("pane.list", json!({}))),
        ["workspace", "focus", id] => Some(("workspace.focus", json!({"workspace_id": id}))),
        ["tab", "focus", id] => Some(("tab.focus", json!({"tab_id": id}))),
        ["pane", "zoom", id, "--off"] => Some(("pane.zoom", json!({"pane_id": id, "mode": "off"}))),
        ["pane", "process-info", "--pane", id] => {
            Some(("pane.process_info", json!({"pane_id": id})))
        }
        ["tab", "list", "--workspace", id] => Some(("tab.list", json!({"workspace_id": id}))),
        _ => None,
    }
}

/// Send a JSON-RPC 2.0 request via UDS and return the `result` field.
fn uds_request(method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
    let path = sock_path().context("HERDR_SOCKET_PATH not set")?;
    let mut stream =
        UnixStream::connect(&path).with_context(|| format!("UDS connect to {path}"))?;

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "req_1",
        "method": method,
        "params": params,
    });

    let mut line = serde_json::to_string(&request)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;

    let mut reader = BufReader::new(&stream);
    let mut response_line = String::new();
    reader.read_line(&mut response_line)?;

    if response_line.is_empty() {
        anyhow::bail!("UDS: empty response from {method}");
    }

    let response: serde_json::Value = serde_json::from_str(&response_line)
        .with_context(|| format!("UDS: invalid JSON from {method}"))?;

    if let Some(err) = response.get("error") {
        anyhow::bail!("UDS RPC error on {method}: {err:?}");
    }

    response
        .get("result")
        .cloned()
        .with_context(|| format!("UDS: response missing result for {method}"))
}

/// Run a herdr CLI command and parse the JSON result.
/// Tries UDS JSON-RPC first, falls back to subprocess CLI.
fn herdr_cli<R: DeserializeOwned>(args: &[&str]) -> Result<R> {
    // Test mode: use mock outputs
    #[cfg(test)]
    {
        if let Some(output) = mock_io::pop_mock_output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!(
                    "mock herdr failed (exit={}): {}",
                    output.status,
                    stderr.trim()
                );
            }
            let response: CliResponse<R> = serde_json::from_str(&stdout).with_context(|| {
                format!("Mock parse failed: {}", &stdout[..stdout.len().min(200)])
            })?;
            return Ok(response.result);
        }
    }

    // Try UDS JSON-RPC first (faster: no process spawn)
    if let Some((method, params)) = args_to_method(args)
        && let Ok(val) = uds_request(method, params)
        && let Ok(result) = serde_json::from_value(val)
    {
        return Ok(result);
    }

    let bin = herdr_bin();
    let output = Command::new(&bin)
        .args(args)
        .output()
        .with_context(|| format!("Failed to run {} {}", bin, args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "{} {} failed (exit={}): {}",
            bin,
            args.join(" "),
            output.status,
            stderr.trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let response: CliResponse<R> = serde_json::from_str(&stdout).with_context(|| {
        format!(
            "Failed to parse herdr CLI output: {}",
            &stdout[..stdout.len().min(200)]
        )
    })?;

    Ok(response.result)
}

/// Fetch the currently focused pane's info from Herdr.
/// Returns (pane_id, tab_id, workspace_id) of the focused pane.
/// This is used by the track subcommand to capture the current focus state
/// when Herdr events don't fire (e.g., intra-workspace tab switches).
pub fn fetch_focused_pane_info() -> Result<(String, String, String, String)> {
    let result: PaneListResult = herdr_cli(&["pane", "list"])?;
    let focused = result
        .panes
        .iter()
        .find(|p| p.focused)
        .context("No focused pane in pane list")?;
    let label = focused
        .label
        .clone()
        .or_else(|| focused.title.clone())
        .unwrap_or_default();
    Ok((
        focused.pane_id.clone(),
        focused.tab_id.clone(),
        focused.workspace_id.clone(),
        label,
    ))
}

/// Fetch the label for a given tab_id.
pub fn fetch_tab_name(tab_id: &str, workspace_id: &str) -> Option<String> {
    herdr_cli::<TabListResult>(&["tab", "list", "--workspace", workspace_id])
        .ok()
        .and_then(|r| {
            r.tabs
                .into_iter()
                .find(|t| t.tab_id == tab_id)
                .and_then(|t| t.label)
        })
}

/// Fetch the label for a given workspace_id.
pub fn fetch_workspace_name(workspace_id: &str) -> Option<String> {
    herdr_cli::<WorkspaceListResult>(&["workspace", "list"])
        .ok()
        .and_then(|r| {
            r.workspaces
                .into_iter()
                .find(|w| w.workspace_id == workspace_id)
                .map(|w| w.label)
        })
}

/// Fetch all navigation nodes from Herdr via CLI.
///
/// Performance: uses bulk `herdr pane list` and `herdr tab list` (no
/// `--workspace` filter) to fetch all data in **3 subprocess calls**
/// regardless of workspace count, instead of 1+2W calls.
///
/// Also returns info about the currently-focused pane (if any),
/// which is used to identify and exclude the navigator's own pane
/// and to seed MRU state — no extra subprocess needed.
pub fn fetch_all_nodes() -> Result<(Vec<NavigationNode>, Option<FocusedPaneInfo>)> {
    // ── 3 subprocess calls total, independent of workspace count ──
    let ws_result: WorkspaceListResult = herdr_cli(&["workspace", "list"])?;

    // Bulk fetch all tabs and all panes in one call each.
    let all_tabs: Vec<TabInfo> = herdr_cli::<TabListResult>(&["tab", "list"])
        .ok()
        .map(|r| r.tabs)
        .unwrap_or_default();

    let all_panes: Vec<PaneInfo> = herdr_cli::<PaneListResult>(&["pane", "list"])
        .ok()
        .map(|r| r.panes)
        .unwrap_or_default();

    // ── Build local lookup maps ──
    let ws_labels = workspace_labels(&ws_result.workspaces);

    let tab_names: HashMap<(String, String), String> = all_tabs
        .into_iter()
        .map(|t| {
            let label = t.label.unwrap_or_else(|| {
                let short = t.tab_id.rsplit(':').next().unwrap_or(&t.tab_id);
                format!("tab-{}", short)
            });
            ((t.workspace_id, t.tab_id), label)
        })
        .collect();

    // ── Build nodes ──
    let mut nodes = Vec::with_capacity(all_panes.len());
    let mut active_pane_info: Option<FocusedPaneInfo> = None;

    for pane in all_panes {
        if pane.focused {
            let label = pane
                .label
                .clone()
                .or_else(|| pane.title.clone())
                .unwrap_or_default();
            active_pane_info = Some(FocusedPaneInfo {
                pane_id: pane.pane_id.clone(),
                tab_id: pane.tab_id.clone(),
                workspace_id: pane.workspace_id.clone(),
                label,
            });
        }

        let agent_status = pane
            .agent_status
            .map(|s| s.into())
            .unwrap_or(AgentStatus::None);

        let pane_name = pane
            .label
            .clone()
            .or_else(|| pane.title.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "untitled".into());

        let tab_name = tab_names
            .get(&(pane.workspace_id.clone(), pane.tab_id.clone()))
            .cloned()
            .unwrap_or_else(|| {
                let short = pane.tab_id.rsplit(':').next().unwrap_or(&pane.tab_id);
                format!("tab-{}", short)
            });

        nodes.push(NavigationNode {
            workspace_id: pane.workspace_id.clone(),
            workspace_name: ws_labels
                .get(&pane.workspace_id)
                .cloned()
                .unwrap_or_default(),
            tab_id: pane.tab_id.clone(),
            tab_name,
            pane_id: pane.pane_id.clone(),
            pane_name: Some(pane_name),
            agent_id: pane.agent.clone(),
            agent_status,
            last_accessed_at: 0,
        });
    }

    Ok((nodes, active_pane_info))
}

/// The recognized foreground command line of a pane.
#[derive(Debug, Clone)]
pub struct PaneProcess {
    pub command: String,
}

/// Fetch the foreground command line of a pane via `herdr pane process-info`.
/// Returns `None` when the pane has no foreground process or an empty command.
pub fn fetch_pane_process(pane_id: &str) -> Result<Option<PaneProcess>> {
    let result: ProcessInfoResult = herdr_cli(&["pane", "process-info", "--pane", pane_id])?;
    let command = result
        .process_info
        .and_then(|pi| pi.foreground_processes.into_iter().next())
        .and_then(|pr| {
            pr.cmdline
                .filter(|s| !s.trim().is_empty())
                .or_else(|| pr.argv.filter(|a| !a.is_empty()).map(|a| a.join(" ")))
                .or_else(|| pr.name.filter(|s| !s.trim().is_empty()))
        })
        .unwrap_or_default();
    if command.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(PaneProcess { command }))
    }
}

/// Fetch the current working directory of every pane in one `pane list` call.
/// Prefers the shell's `cwd`, falling back to `foreground_cwd`.
fn fetch_cwd_map() -> Result<HashMap<String, String>> {
    let result: PaneListResult = herdr_cli(&["pane", "list"])?;
    Ok(result
        .panes
        .into_iter()
        .filter_map(|p| {
            let cwd = p.cwd.or(p.foreground_cwd)?;
            Some((p.pane_id, cwd))
        })
        .collect())
}

/// Refresh the lazy "All" state (cwd / command / ssh target) for non-agent
/// panes. `map` is mutated in place and may be pre-seeded (e.g. from a prior
/// refresh) — existing command/ssh values are preserved to avoid re-polling.
///
/// Cost: one bulk `pane list` + one `pane process-info` per pane without a
/// cached command. Callers run this in a background thread, only while the
/// All tab is active.
pub fn refresh_others(
    nodes: &[NavigationNode],
    map: &mut HashMap<String, PaneOthers>,
) -> Result<usize> {
    // Refresh cwd for all panes in one bulk call.
    let cwd_map = fetch_cwd_map()?;
    for n in nodes {
        if n.agent_id.is_some() {
            continue;
        }
        let entry = map.entry(n.pane_id.clone()).or_default();
        if let Some(cwd) = cwd_map.get(&n.pane_id) {
            entry.cwd = Some(cwd.clone());
        }
    }

    // Lazily fill command/ssh target (once per pane) from process-info.
    let mut updated = 0;
    for n in nodes {
        if n.agent_id.is_some() {
            continue;
        }
        let entry = map.entry(n.pane_id.clone()).or_default();
        if entry.command.is_some() {
            continue;
        }
        if let Ok(Some(proc)) = fetch_pane_process(&n.pane_id) {
            entry.command = Some(proc.command.clone());
            entry.ssh_target = crate::others::parse_ssh_target(&proc.command);
            updated += 1;
        }
    }
    Ok(updated)
}

/// Maximum buffered characters per pane kept for content search.
/// Bounds the per-keystroke scan cost; herdr buffers are typically much smaller.
const CONTENT_CAP: usize = 256 * 1024;

/// Run a herdr CLI command and return the raw stdout (used for `pane read`,
/// whose output is terminal text, not a JSON envelope).
/// Prefers the same subprocess path as `herdr_cli`; UDS is not used because
/// the CLI prints the pane buffer verbatim without a JSON result wrapper.
fn herdr_cli_raw(args: &[&str]) -> Result<String> {
    #[cfg(test)]
    {
        if let Some(output) = mock_io::pop_mock_output() {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!(
                    "mock herdr failed (exit={}): {}",
                    output.status,
                    stderr.trim()
                );
            }
            return Ok(stdout);
        }
    }

    let bin = herdr_bin();
    let output = Command::new(&bin)
        .args(args)
        .output()
        .with_context(|| format!("Failed to run {} {}", bin, args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "{} {} failed (exit={}): {}",
            bin,
            args.join(" "),
            output.status,
            stderr.trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Fetch a pane's terminal buffer as ANSI-stripped text via `herdr pane read`.
/// Returns `None` when the pane has no readable scrollback.
pub fn fetch_pane_content(pane_id: &str) -> Result<Option<String>> {
    let raw = herdr_cli_raw(&[
        "pane",
        "read",
        pane_id,
        "--source",
        "recent-unwrapped",
    ])?;
    let stripped = crate::others::strip_ansi(&raw);
    if stripped.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(stripped))
    }
}

/// Refresh cached pane content for content search. Fetches only panes missing
/// from `map` (existing buffers are preserved); agent panes and the navigator's
/// own pane are skipped, mirroring the Others-tab exclusions.
pub fn refresh_contents(
    nodes: &[NavigationNode],
    map: &mut HashMap<String, String>,
    self_pane_id: Option<&str>,
) -> Result<usize> {
    let mut updated = 0;
    for n in nodes {
        if n.agent_id.is_some() {
            continue;
        }
        if self_pane_id.is_some_and(|s| s == n.pane_id) {
            continue;
        }
        if map.contains_key(&n.pane_id) {
            continue;
        }
        if let Ok(Some(content)) = fetch_pane_content(&n.pane_id) {
            // Cap on a char boundary: String::truncate would panic on CJK.
            let content = if content.chars().count() > CONTENT_CAP {
                content.chars().take(CONTENT_CAP).collect()
            } else {
                content
            };
            map.insert(n.pane_id.clone(), content);
            updated += 1;
        }
    }
    Ok(updated)
}

/// Focus a specific workspace via herdr CLI.
pub fn focus_workspace(workspace_id: &str) -> Result<()> {
    run_focus(&["workspace", "focus", workspace_id])
}

/// Focus a specific tab via herdr CLI.
pub fn focus_tab(tab_id: &str) -> Result<()> {
    run_focus(&["tab", "focus", tab_id])
}

/// Focus a specific pane via herdr CLI.
/// Uses `pane zoom --off` to avoid the toggle-zoom behavior — the pane
/// gets focused but never enters zoomed/maximized state.
pub fn focus_pane(pane_id: &str) -> Result<()> {
    run_focus(&["pane", "zoom", pane_id, "--off"])
}

fn run_focus(args: &[&str]) -> Result<()> {
    let _: serde_json::Value = herdr_cli(args)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::mock_io;
    use super::*;
    use serial_test::serial;

    // ── fetch_all_nodes ──

    #[test]
    #[serial]
    fn test_fetch_all_nodes_parses_valid_response() {
        mock_io::clear();
        let ws = mock_io::make_output(
            r#"{"result":{"workspaces":[{"workspace_id":"ws-1","label":"MyWS"}]}}"#,
        );
        let tabs = mock_io::make_output(
            r#"{"result":{"tabs":[{"tab_id":"tab-1","workspace_id":"ws-1","label":"MyTab"}]}}"#,
        );
        let panes = mock_io::make_output(
            r#"{"result":{"panes":[{"pane_id":"pane-1","workspace_id":"ws-1","tab_id":"tab-1","focused":false,"label":"MyPane"}]}}"#,
        );
        mock_io::set_mock_outputs(vec![ws, tabs, panes]);

        let (nodes, focused) = fetch_all_nodes().unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].workspace_name, "MyWS");
        assert_eq!(nodes[0].tab_name, "MyTab");
        assert_eq!(nodes[0].pane_name.as_deref(), Some("MyPane"));
        assert!(focused.is_none(), "No focused pane in this test");
    }

    #[test]
    #[serial]
    fn test_fetch_all_nodes_empty_lists() {
        mock_io::clear();
        let ws = mock_io::make_output(r#"{"result":{"workspaces":[]}}"#);
        let tabs = mock_io::make_output(r#"{"result":{"tabs":[]}}"#);
        let panes = mock_io::make_output(r#"{"result":{"panes":[]}}"#);
        mock_io::set_mock_outputs(vec![ws, tabs, panes]);

        let (nodes, focused) = fetch_all_nodes().unwrap();
        assert!(nodes.is_empty(), "Empty lists should produce empty nodes");
        assert!(focused.is_none());
    }

    #[test]
    #[serial]
    fn test_fetch_all_nodes_missing_fields_use_defaults() {
        mock_io::clear();
        let ws = mock_io::make_output(
            r#"{"result":{"workspaces":[{"workspace_id":"ws-1","label":"WS"}]}}"#,
        );
        let tabs = mock_io::make_output(
            r#"{"result":{"tabs":[{"tab_id":"tab-1","workspace_id":"ws-1"}]}}"#,
        );
        let panes = mock_io::make_output(
            r#"{"result":{"panes":[{"pane_id":"pane-1","workspace_id":"ws-1","tab_id":"tab-1","focused":false}]}}"#,
        );
        mock_io::set_mock_outputs(vec![ws, tabs, panes]);

        let (nodes, _) = fetch_all_nodes().unwrap();
        assert_eq!(nodes[0].pane_name.as_deref(), Some("untitled"));
    }

    #[test]
    #[serial]
    fn test_fetch_all_nodes_detects_focused_pane() {
        mock_io::clear();
        let ws = mock_io::make_output(
            r#"{"result":{"workspaces":[{"workspace_id":"ws-1","label":"WS"}]}}"#,
        );
        let tabs = mock_io::make_output(
            r#"{"result":{"tabs":[{"tab_id":"tab-1","workspace_id":"ws-1","label":"ActiveTab"}]}}"#,
        );
        let panes = mock_io::make_output(
            r#"{"result":{"panes":[{"pane_id":"pane-1","workspace_id":"ws-1","tab_id":"tab-1","focused":true,"label":"ActivePane"}]}}"#,
        );
        mock_io::set_mock_outputs(vec![ws, tabs, panes]);

        let (_, focused) = fetch_all_nodes().unwrap();
        assert!(focused.is_some());
        assert_eq!(focused.unwrap().pane_id, "pane-1");
    }

    // ── fetch_focused_pane_info ──

    #[test]
    #[serial]
    fn test_fetch_focused_pane_info_no_focused_returns_error() {
        mock_io::clear();
        let panes =
            mock_io::make_output(r#"{"result":{"panes":[{"pane_id":"p-1","focused":false}]}}"#);
        mock_io::set_mock_outputs(vec![panes]);
        let result = fetch_focused_pane_info();
        assert!(result.is_err(), "No focused pane should return error");
    }

    // ── focus commands ──

    #[test]
    #[serial]
    fn test_focus_workspace_success() {
        mock_io::clear();
        let output = mock_io::make_output(r#"{"result":{}}"#);
        mock_io::set_mock_outputs(vec![output]);
        let result = focus_workspace("ws-1");
        assert!(result.is_ok());
    }

    #[test]
    #[serial]
    fn test_focus_workspace_failure() {
        mock_io::clear();
        mock_io::set_mock_outputs(vec![mock_io::make_failing_output()]);
        let result = focus_workspace("ws-1");
        assert!(result.is_err());
    }

    // ── process-info / others ──

    #[test]
    #[serial]
    fn test_fetch_pane_process_parses_cmdline() {
        mock_io::clear();
        let pi = mock_io::make_output(
            r#"{"result":{"process_info":{"foreground_processes":[{"argv":["psql","-U","poste"],"cmdline":"psql -U poste -h 127.0.0.1","name":"psql","pid":1}],"pane_id":"p-1","shell_pid":2}}}"#,
        );
        mock_io::set_mock_outputs(vec![pi]);
        let proc = fetch_pane_process("p-1").unwrap().unwrap();
        assert_eq!(proc.command, "psql -U poste -h 127.0.0.1");
    }

    #[test]
    #[serial]
    fn test_fetch_pane_process_empty_returns_none() {
        mock_io::clear();
        let pi = mock_io::make_output(
            r#"{"result":{"process_info":{"foreground_processes":[],"pane_id":"p-1"}}}"#,
        );
        mock_io::set_mock_outputs(vec![pi]);
        assert!(fetch_pane_process("p-1").unwrap().is_none());
    }

    #[test]
    #[serial]
    fn test_fetch_cwd_map_prefers_shell_cwd() {
        mock_io::clear();
        let panes = mock_io::make_output(
            r#"{"result":{"panes":[
                {"pane_id":"p-1","workspace_id":"ws-1","tab_id":"t-1","focused":false,"cwd":"/a","foreground_cwd":"/a"},
                {"pane_id":"p-2","workspace_id":"ws-1","tab_id":"t-1","focused":false,"cwd":"/b","foreground_cwd":"/b"},
                {"pane_id":"p-3","workspace_id":"ws-1","tab_id":"t-1","focused":false,"foreground_cwd":"/only-fg"}
            ]}}"#,
        );
        mock_io::set_mock_outputs(vec![panes]);
        let map = fetch_cwd_map().unwrap();
        assert_eq!(map.get("p-1").map(String::as_str), Some("/a"));
        assert_eq!(map.get("p-2").map(String::as_str), Some("/b"));
        assert_eq!(map.get("p-3").map(String::as_str), Some("/only-fg"));
    }

    #[test]
    #[serial]
    fn test_refresh_others_fills_command_ssh_cwd() {
        mock_io::clear();
        // 1 pane list + process-info per non-agent pane
        let panes = mock_io::make_output(
            r#"{"result":{"panes":[
                {"pane_id":"p-shell","workspace_id":"ws-1","tab_id":"t-1","focused":false,"cwd":"/home/lex","foreground_cwd":"/home/lex"},
                {"pane_id":"p-ssh","workspace_id":"ws-1","tab_id":"t-1","focused":false,"cwd":"/work","foreground_cwd":"/work"},
                {"pane_id":"p-agent","workspace_id":"ws-1","tab_id":"t-1","focused":false,"agent":"claude","cwd":"/agentws","foreground_cwd":"/agentws"}
            ]}}"#,
        );
        let shell = mock_io::make_output(
            r#"{"result":{"process_info":{"foreground_processes":[{"cmdline":"-zsh","name":"zsh"}],"pane_id":"p-shell"}}}"#,
        );
        let ssh = mock_io::make_output(
            r#"{"result":{"process_info":{"foreground_processes":[{"cmdline":"ssh -p 2200 lex@10.0.0.9","name":"ssh"}],"pane_id":"p-ssh"}}}"#,
        );
        mock_io::set_mock_outputs(vec![panes, shell, ssh]);

        let nodes = vec![
            crate::models::NavigationNode {
                workspace_id: "ws-1".into(),
                workspace_name: "w".into(),
                tab_id: "t-1".into(),
                tab_name: "t".into(),
                pane_id: "p-shell".into(),
                pane_name: Some("shell".into()),
                agent_id: None,
                agent_status: crate::models::AgentStatus::None,
                last_accessed_at: 0,
            },
            crate::models::NavigationNode {
                workspace_id: "ws-1".into(),
                workspace_name: "w".into(),
                tab_id: "t-1".into(),
                tab_name: "t".into(),
                pane_id: "p-ssh".into(),
                pane_name: Some("ssh".into()),
                agent_id: None,
                agent_status: crate::models::AgentStatus::None,
                last_accessed_at: 0,
            },
            crate::models::NavigationNode {
                workspace_id: "ws-1".into(),
                workspace_name: "w".into(),
                tab_id: "t-1".into(),
                tab_name: "t".into(),
                pane_id: "p-agent".into(),
                pane_name: Some("agent".into()),
                agent_id: Some("claude".into()),
                agent_status: crate::models::AgentStatus::Working,
                last_accessed_at: 0,
            },
        ];

        let mut map = std::collections::HashMap::new();
        let updated = refresh_others(&nodes, &mut map).unwrap();
        // Agent pane is skipped; shell and ssh both got a command.
        assert_eq!(updated, 2);
        let shell_entry = map.get("p-shell").unwrap();
        assert_eq!(shell_entry.cwd.as_deref(), Some("/home/lex"));
        assert_eq!(shell_entry.command.as_deref(), Some("-zsh"));
        assert_eq!(shell_entry.ssh_target, None);
        let ssh_entry = map.get("p-ssh").unwrap();
        assert_eq!(ssh_entry.cwd.as_deref(), Some("/work"));
        assert_eq!(ssh_entry.command.as_deref(), Some("ssh -p 2200 lex@10.0.0.9"));
        assert_eq!(ssh_entry.ssh_target.as_deref(), Some("lex@10.0.0.9:2200"));
        // Agent panes never get entries.
        assert!(!map.contains_key("p-agent"));
    }

    // ── pane content (content search) ──

    #[test]
    #[serial]
    fn test_fetch_pane_content_strips_ansi_and_trims() {
        mock_io::clear();
        let raw = mock_io::make_output("\x1b[38;5;1mline one\x1b[0m\nline two\n");
        mock_io::set_mock_outputs(vec![raw]);
        let content = fetch_pane_content("p-1").unwrap().unwrap();
        assert_eq!(content, "line one\nline two\n");
    }

    #[test]
    #[serial]
    fn test_fetch_pane_content_empty_returns_none() {
        mock_io::clear();
        mock_io::set_mock_outputs(vec![mock_io::make_output("   \n \x1b[0m  \n")]);
        assert!(fetch_pane_content("p-1").unwrap().is_none());
    }

    #[test]
    #[serial]
    fn test_refresh_contents_skips_cached_agent_and_self() {
        mock_io::clear();
        let nodes = vec![
            // p-shell fetched, p-agent skipped, self skipped, p-cached preserved
            crate::models::NavigationNode {
                workspace_id: "ws-1".into(),
                workspace_name: "w".into(),
                tab_id: "t-1".into(),
                tab_name: "t".into(),
                pane_id: "p-shell".into(),
                pane_name: Some("shell".into()),
                agent_id: None,
                agent_status: crate::models::AgentStatus::None,
                last_accessed_at: 0,
            },
            crate::models::NavigationNode {
                workspace_id: "ws-1".into(),
                workspace_name: "w".into(),
                tab_id: "t-1".into(),
                tab_name: "t".into(),
                pane_id: "p-agent".into(),
                pane_name: Some("agent".into()),
                agent_id: Some("claude".into()),
                agent_status: crate::models::AgentStatus::Working,
                last_accessed_at: 0,
            },
            crate::models::NavigationNode {
                workspace_id: "ws-1".into(),
                workspace_name: "w".into(),
                tab_id: "t-1".into(),
                tab_name: "t".into(),
                pane_id: "self".into(),
                pane_name: Some("navigator".into()),
                agent_id: None,
                agent_status: crate::models::AgentStatus::None,
                last_accessed_at: 0,
            },
            crate::models::NavigationNode {
                workspace_id: "ws-1".into(),
                workspace_name: "w".into(),
                tab_id: "t-1".into(),
                tab_name: "t".into(),
                pane_id: "p-cached".into(),
                pane_name: Some("cached".into()),
                agent_id: None,
                agent_status: crate::models::AgentStatus::None,
                last_accessed_at: 0,
            },
        ];
        // Exactly one fetch: for p-shell. cached/agent/self never fetch.
        mock_io::set_mock_outputs(vec![mock_io::make_output("buffered text\n")]);

        let mut map = std::collections::HashMap::new();
        map.insert("p-cached".to_string(), "old cached content".to_string());
        let updated = refresh_contents(&nodes, &mut map, Some("self")).unwrap();
        assert_eq!(updated, 1);
        assert_eq!(map.get("p-shell").map(String::as_str), Some("buffered text\n"));
        assert_eq!(map.get("p-cached").map(String::as_str), Some("old cached content"));
        assert!(!map.contains_key("p-agent"));
        assert!(!map.contains_key("self"));

        // A second pass performs zero fetches (all cached now).
        mock_io::clear();
        let updated = refresh_contents(&nodes, &mut map, Some("self")).unwrap();
        assert_eq!(updated, 0);
    }

    // ── RPC method mapping ──

    #[test]
    fn test_args_to_method_process_info() {
        let mapped = args_to_method(&["pane", "process-info", "--pane", "w1:p2"]);
        assert!(mapped.is_some());
        let (method, params) = mapped.unwrap();
        assert_eq!(method, "pane.process_info");
        assert_eq!(params, serde_json::json!({"pane_id": "w1:p2"}));
    }
}
