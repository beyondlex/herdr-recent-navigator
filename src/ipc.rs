use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::models::{AgentStatus, NavigationNode};

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
        ["pane", "zoom", id, "--off"] => Some(("pane.zoom", json!({"pane_id": id, "off": true}))),
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
    if let Some((method, params)) = args_to_method(args) {
        if let Ok(val) = uds_request(method, params) {
            if let Ok(result) = serde_json::from_value(val) {
                return Ok(result);
            }
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
    let ws_labels: HashMap<String, String> = ws_result
        .workspaces
        .iter()
        .map(|w| (w.workspace_id.clone(), w.label.clone()))
        .collect();

    let tab_names: HashMap<(String, String), String> = all_tabs
        .into_iter()
        .filter_map(|t| {
            let label = t.label.unwrap_or_else(|| {
                let short = t.tab_id.rsplit(':').next().unwrap_or(&t.tab_id);
                format!("tab-{}", short)
            });
            Some(((t.workspace_id, t.tab_id), label))
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
}
