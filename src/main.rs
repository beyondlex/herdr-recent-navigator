mod app;
mod cli;
mod data;
mod format;
mod ipc;
mod models;
mod mru;
mod tracker;
mod ui;

#[cfg(test)]
mod test_helpers;

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{self, stdout};
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use crossterm::terminal::disable_raw_mode;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use cli::{Cli, Command as CliCommand};
use models::{AppState, CategoryTab, FocusTarget, KeyAction};

fn pane_lock_path() -> PathBuf {
    std::env::temp_dir().join("herdr-recent-navigator").join("pane.lock")
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // ── Logger setup ──
    {
        let mut builder = env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or("herdr_recent_navigator=info"),
        );
        builder.format_timestamp_millis();
        if let Some(path) = &cli.log_file {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .context("Failed to open --log-file")?;
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        }
        builder.init();
    }

    // ── Track mode ──
    if let Some(CliCommand::Track) = &cli.command {
        return handle_track();
    }

    // ── --pane-open mode — toggle the overlay pane ──
    if cli.pane_open {
        // If --view is also set, save the category so the pane starts on that tab
        if let Some(view) = &cli.view {
            // Use a dummy AppState to write the category to state.json
            if let Ok(cat) = view.parse::<CategoryTab>() {
                let state = AppState::new(vec![]);
                state.save_category(&cat);
            }
        }
        return handle_pane_open();
    }

    // ── Normal (pane) mode: delegate to run_inner for data + state init ──
    let (mut state, pane_ts, tab_ts, ws_ts, ctx, connected) = run_inner(&cli)?;

    // ── Terminal setup ──
    // The pane runs as an overlay inside Herdr. Uses raw stdin/stdout directly.
    crossterm::terminal::enable_raw_mode()?;
    let mut terminal = {
        let backend = CrosstermBackend::new(stdout());
        Terminal::new(backend)?
    };

    // ── Event Loop ──
    let selected_target = run_event_loop(
        &mut terminal,
        &mut state,
        &pane_ts,
        &tab_ts,
        &ws_ts,
        &ctx,
        connected,
    )?;

    // ── Terminal cleanup ──
    // Clean up the lock file so the next shortcut press opens, not closes.
    if let Err(e) = std::fs::remove_file(pane_lock_path()) {
        log::debug!("Failed to remove pane lock file: {e}");
    }
    // Write a cleanup sequence directly so the pane overlay restores cleanly.
    {
        use std::io::Write;
        let mut out = stdout();
        if let Err(e) = write!(out, "\x1b[?25h\x1b[0m") {
            log::warn!("Failed to write cleanup sequence: {e}");
        }
        if let Err(e) = out.flush() {
            log::warn!("Failed to flush stdout: {e}");
        }
    }
    disable_raw_mode()?;

    // ── Post-exit: focus the selected entity (connected mode) ──
    if connected && let Some(target) = selected_target {
        let result = match target {
            FocusTarget::Workspace(id) => ipc::focus_workspace(&id),
            FocusTarget::Tab(id) => ipc::focus_tab(&id),
            FocusTarget::Pane(pane_id) => ipc::focus_pane(&pane_id),
        };
        if let Err(e) = result {
            log::error!("Failed to focus: {}", e);
        }
    }

    Ok(())
}

/// Non-TUI core logic: data fetching, state initialization, MRU recording.
/// Returns fully initialized state + timestamp maps + context for the TUI event loop.
/// This function is testable without a terminal — it does no I/O beyond IPC/state files.
fn run_inner(
    cli: &Cli,
) -> Result<(
    AppState,
    HashMap<String, u64>, // pane_ts
    HashMap<String, u64>, // tab_ts
    HashMap<String, u64>, // ws_ts
    ActiveContext,
    bool, // connected
)> {
    #[cfg(feature = "mock")]
    let (nodes, focused_pane_info, connected) = {
        if cli.mock {
            (data::mock_nodes(), None, false)
        } else {
            match ipc::fetch_all_nodes() {
                Ok((nodes, info)) => (nodes, info, true),
                Err(e) => {
                    log::warn!("Herdr CLI failed ({}), falling back to mock mode", e);
                    (data::mock_nodes(), None, false)
                }
            }
        }
    };

    #[cfg(not(feature = "mock"))]
    let (nodes, focused_pane_info, connected) = match ipc::fetch_all_nodes() {
        Ok((nodes, info)) => (nodes, info, true),
        Err(e) => return Err(anyhow::anyhow!("Failed to connect to Herdr: {e}")),
    };

    // Record current focus at navigator startup (single pass over nodes)
    if connected && let Some(fpi) = &focused_pane_info {
        let mut pane_name = if fpi.label.is_empty() {
            None
        } else {
            Some(fpi.label.clone())
        };
        let mut tab_name: Option<String> = None;
        let mut ws_name: Option<String> = None;
        for n in &nodes {
            if pane_name.is_none() && n.pane_id == fpi.pane_id {
                pane_name.clone_from(&n.pane_name);
            }
            if tab_name.is_none() && n.tab_id == fpi.tab_id {
                tab_name = Some(n.tab_name.clone());
            }
            if ws_name.is_none() && n.workspace_id == fpi.workspace_id {
                ws_name = Some(n.workspace_name.clone());
            }
            if pane_name.is_some() && tab_name.is_some() && ws_name.is_some() {
                break;
            }
        }

        if let Err(e) = tracker::record_event_with_names(
            tracker::MruKind::Pane,
            &fpi.pane_id,
            &fpi.workspace_id,
            pane_name,
            ws_name.clone(),
        ) {
            log::error!("Failed to record pane focus event: {e}");
        }
        if let Err(e) = tracker::record_event_with_names(
            tracker::MruKind::Tab,
            &fpi.tab_id,
            &fpi.workspace_id,
            tab_name,
            ws_name.clone(),
        ) {
            log::error!("Failed to record tab focus event: {e}");
        }
        if let Err(e) = tracker::record_event_with_names(
            tracker::MruKind::Workspace,
            &fpi.workspace_id,
            &fpi.workspace_id,
            ws_name,
            None,
        ) {
            log::error!("Failed to record workspace focus event: {e}");
        }
    }

    let mru_entries = tracker::load_mru();
    let (pane_ts, tab_ts, ws_ts) = tracker::build_timestamp_maps(&mru_entries);

    let ctx = derive_active_context(
        &nodes,
        focused_pane_info.as_ref().map(|f| f.pane_id.clone()),
    );

    let mut state = AppState::new(nodes);
    state.theme_name = ctx.theme_name.clone();

    if let Some(last) = AppState::load_last_category() {
        state.current_category = last;
    }
    if let Some(view) = &cli.view {
        if let Ok(cat) = view.parse::<CategoryTab>() {
            state.current_category = cat;
        }
    }

    Ok((state, pane_ts, tab_ts, ws_ts, ctx, connected))
}

/// Context derived from `HERDR_PLUGIN_CONTEXT_JSON` about the currently
/// focused entities. Used to exclude the "current" entity from its
/// corresponding category tab.
struct ActiveContext {
    workspace_id: Option<String>,
    pane_id: Option<String>,
    tab_id: Option<String>,
    self_pane_id: Option<String>,
    theme_name: Option<String>,
}

/// Derive the currently focused workspace/pane/tab from environment context.
/// `ipc_active_pane_id` is the pane with `focused: true` from the IPC pane
/// list response (the navigator's own overlay pane).
fn derive_active_context(
    nodes: &[models::NavigationNode],
    ipc_active_pane_id: Option<String>,
) -> ActiveContext {
    let context_json = match std::env::var("HERDR_PLUGIN_CONTEXT_JSON").ok() {
        Some(j) => j,
        None => {
            return ActiveContext {
                workspace_id: None,
                pane_id: None,
                tab_id: None,
                self_pane_id: ipc_active_pane_id,
                theme_name: None,
            };
        }
    };
    let v: serde_json::Value = match serde_json::from_str(&context_json).ok() {
        Some(val) => val,
        None => {
            return ActiveContext {
                workspace_id: None,
                pane_id: None,
                tab_id: None,
                self_pane_id: ipc_active_pane_id,
                theme_name: None,
            };
        }
    };

    let workspace_id = v
        .get("focused_workspace_id")
        .or_else(|| v.get("workspace_id"))
        .and_then(|id| id.as_str())
        .map(String::from);

    let pane_id = v
        .get("focused_pane_id")
        .and_then(|id| id.as_str())
        .map(String::from);

    // Derive tab_id by finding the node whose pane_id matches the focused pane
    let tab_id = pane_id.as_deref().and_then(|pid| {
        nodes
            .iter()
            .find(|n| n.pane_id == pid)
            .map(|n| n.tab_id.clone())
    });

    let theme_name = v
        .get("theme_name")
        .and_then(|t| t.as_str())
        .map(String::from);

    ActiveContext {
        workspace_id,
        pane_id,
        tab_id,
        self_pane_id: ipc_active_pane_id,
        theme_name,
    }
}

/// Handle the `track` subcommand.
///
/// Herdr guarantees `workspace.focused` events but may not fire
/// `pane.focused` or `tab.focused` for every tab/pane switch within a
/// workspace. To compensate, this function does two things:
///
/// 1. Records the event data (workspace/pane/tab as appropriate)
/// 2. **Additionally** polls the current focused pane via IPC and records
///    the pane, tab, and workspace from the live Herdr state.
///
/// This ensures that even when specific events don't fire, the "trigger
/// effect" of any event captures the full current focus state.
///
/// Event JSON shape (plugin hook format):
///   {"event":"pane_focused","data":{"type":"pane_focused","pane_id":"...","workspace_id":"..."}}
fn handle_track() -> Result<()> {
    let event_json =
        std::env::var("HERDR_PLUGIN_EVENT_JSON").context("HERDR_PLUGIN_EVENT_JSON not set")?;
    let v: serde_json::Value =
        serde_json::from_str(&event_json).context("Failed to parse HERDR_PLUGIN_EVENT_JSON")?;

    let event_name = v
        .get("event")
        .and_then(|e| e.as_str())
        .context("HERDR_PLUGIN_EVENT_JSON missing 'event' field")?;

    let data = v
        .get("data")
        .context("HERDR_PLUGIN_EVENT_JSON missing 'data' field")?;

    match event_name {
        "workspace_focused" => {
            let ws_id = data
                .get("workspace_id")
                .and_then(|s| s.as_str())
                .context("missing workspace_id in workspace.focused data")?;
            tracker::record_event(tracker::MruKind::Workspace, ws_id, ws_id)?;
        }
        "pane_focused" => {
            let pane_id = data
                .get("pane_id")
                .and_then(|s| s.as_str())
                .context("missing pane_id in pane.focused data")?;
            let ws_id = data
                .get("workspace_id")
                .and_then(|s| s.as_str())
                .context("missing workspace_id in pane.focused data")?;
            tracker::record_event(tracker::MruKind::Pane, pane_id, ws_id)?;
        }
        "tab_focused" => {
            let tab_id = data
                .get("tab_id")
                .and_then(|s| s.as_str())
                .context("missing tab_id in tab.focused data")?;
            let ws_id = data
                .get("workspace_id")
                .and_then(|s| s.as_str())
                .context("missing workspace_id in tab.focused data")?;
            tracker::record_event(tracker::MruKind::Tab, tab_id, ws_id)?;
        }
        _ => {
            log::warn!("Ignoring unknown event type: {}", event_name);
        }
    }

    // ── Poll current focused state ──
    // Every event trigger is also an opportunity to capture the full current
    // focus state via IPC, filling gaps when Herdr doesn't emit events for
    // every tab/pane switch within a workspace.
    if let Ok((pane_id, tab_id, ws_id, pane_label)) = ipc::fetch_focused_pane_info() {
        let tab_name = ipc::fetch_tab_name(&tab_id, &ws_id);
        let ws_name = ipc::fetch_workspace_name(&ws_id);
        let pane_name = Some(pane_label).filter(|s| !s.is_empty());

        if let Err(e) = tracker::record_event_with_names(
            tracker::MruKind::Pane,
            &pane_id,
            &ws_id,
            pane_name,
            ws_name.clone(),
        ) {
            log::error!("Track poll: failed to record pane event: {e}");
        }
        if let Err(e) = tracker::record_event_with_names(
            tracker::MruKind::Tab,
            &tab_id,
            &ws_id,
            tab_name,
            ws_name.clone(),
        ) {
            log::error!("Track poll: failed to record tab event: {e}");
        }
        if let Err(e) = tracker::record_event_with_names(
            tracker::MruKind::Workspace,
            &ws_id,
            &ws_id,
            ws_name,
            None,
        ) {
            log::error!("Track poll: failed to record workspace event: {e}");
        }
    }

    Ok(())
}

/// Run the TUI event loop until the user exits.
/// Returns the entity to focus on exit, if any.
fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut AppState,
    pane_ts: &HashMap<String, u64>,
    tab_ts: &HashMap<String, u64>,
    ws_ts: &HashMap<String, u64>,
    ctx: &ActiveContext,
    connected: bool,
) -> Result<Option<FocusTarget>> {
    let mut last_refresh = Instant::now();
    let (refresh_tx, refresh_rx) = mpsc::channel();
    let refresh_in_flight = Arc::new(AtomicBool::new(false));
    loop {
        // ── Periodic data refresh (non-blocking, background thread) ──
        if connected
            && last_refresh.elapsed() >= Duration::from_secs(2)
            && !refresh_in_flight.load(Ordering::Relaxed)
        {
            last_refresh = Instant::now();
            refresh_in_flight.store(true, Ordering::Relaxed);
            let tx = refresh_tx.clone();
            let flag = refresh_in_flight.clone();
            std::thread::spawn(move || {
                // Use catch_unwind so a panic doesn't leave the flag stuck
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    crate::ipc::fetch_all_nodes()
                }));
                match result {
                    Ok(Ok((fresh_nodes, _))) => {
                        let _ = tx.send(fresh_nodes);
                    }
                    Ok(Err(e)) => {
                        log::error!("Background refresh failed: {e}");
                    }
                    Err(_) => {
                        log::error!("Background refresh panicked");
                    }
                }
                flag.store(false, Ordering::Relaxed);
            });
        }
        // Drain any background-fetched data without blocking the render.
        while let Ok(fresh_nodes) = refresh_rx.try_recv() {
            state.nodes = fresh_nodes;
            state.cache_key = None; // nodes changed, invalidate cache
        }

        // ── Build display list once, shared by render + event handler ──
        let opts = mru::BuildOptions {
            pane_ts,
            tab_ts,
            ws_ts,
            active_workspace_id: ctx.workspace_id.as_deref(),
            active_pane_id: ctx.pane_id.as_deref(),
            active_tab_id: ctx.tab_id.as_deref(),
            self_pane_id: ctx.self_pane_id.as_deref(),
        };
        // Use cache key to skip rebuild when input hasn't changed
        let cache_key = mru::build_cache_key(
            &state.nodes,
            &opts,
            &state.current_category,
            &state.search_query,
        );
        let (displayed, total) = if state.cache_key == Some(cache_key) {
            // Cache hit: Rc::clone is O(1), no deep copy needed
            (state.cached_displayed.clone(), state.cached_total)
        } else {
            // Cache miss: rebuild
            let items = mru::build_display_list(&state.nodes, &opts, &state.current_category);
            let total = items.len();
            let displayed = if state.search_query.is_empty() {
                Rc::new(items)
            } else {
                Rc::new(mru::search_display_items(&items, &state.search_query))
            };
            // Update cache
            state.cache_key = Some(cache_key);
            state.cached_total = total;
            state.cached_displayed = displayed.clone();
            (displayed, total)
        };
        let displayed_len = displayed.len();

        // ── Render ──
        state.spinner_tick = state.spinner_tick.wrapping_add(1);
        terminal.draw(|frame| {
            ui::render(frame, state, &*displayed, total);
        })?;

        // ── Poll input non-blocking (50ms timeout drives spinner refresh) ──
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }

        match event::read()? {
            Event::Key(KeyEvent {
                code,
                modifiers,
                kind: KeyEventKind::Press,
                ..
            }) => {
                let key_event = KeyEvent::new(code, modifiers);
                let action = state.handle_key(key_event, displayed_len);

                match action {
                    KeyAction::ExitSelect => {
                        state.save_last_category();
                        // Use the already-built displayed list, avoid rebuilding
                        let target = get_selected_target_from_list(state, &*displayed);
                        return Ok(target);
                    }
                    KeyAction::ExitDismiss => {
                        state.save_last_category();
                        return Ok(None);
                    }
                    KeyAction::Continue => {}
                }
            }
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
}

/// Build the `FocusTarget` for the currently selected item.
/// Uses the already-constructed `displayed` list instead of rebuilding from scratch.
fn get_selected_target_from_list(
    state: &AppState,
    displayed: &[models::DisplayItem],
) -> Option<FocusTarget> {
    displayed.get(state.selected_index).map(|item| match item {
        models::DisplayItem::Workspace { id, .. } => FocusTarget::Workspace(id.clone()),
        models::DisplayItem::Tab { tab_id, .. } => FocusTarget::Tab(tab_id.clone()),
        models::DisplayItem::Agent { pane_id, .. } | models::DisplayItem::Pane { pane_id, .. } => {
            FocusTarget::Pane(pane_id.clone())
        }
    })
}

/// Toggle the navigator overlay pane open/closed.
fn handle_pane_open() -> Result<()> {
    let herdr_bin = std::env::var("HERDR_BIN_PATH")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "herdr".to_string());
    let plugin_id = std::env::var("HERDR_PLUGIN_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "recent-navigator".to_string());

    // If lock file exists, close the existing pane (toggle closed)
    let lock_path = pane_lock_path();
    if let Some(parent) = lock_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(pane_id) = std::fs::read_to_string(&lock_path) {
        let pane_id = pane_id.trim().to_string();
        log::info!("pane-open: closing existing pane {pane_id}");
        let status = ProcessCommand::new(&herdr_bin)
            .args(["plugin", "pane", "close", &pane_id])
            .status();
        if let Err(e) = std::fs::remove_file(&lock_path) {
            log::warn!("Failed to remove lock file: {e}");
        }
        if let Ok(s) = status
            && s.success()
        {
            return Ok(());
        }
        // Stale lock — close failed, fall through to open a fresh pane
        log::warn!("pane-open: stale lock, opening fresh pane");
    }

    // Open a new navigator pane
    let output = ProcessCommand::new(&herdr_bin)
        .args([
            "plugin",
            "pane",
            "open",
            "--plugin",
            &plugin_id,
            "--entrypoint",
            "navigator",
            "--placement",
            "popup",
            "--focus",
        ])
        .output()
        .context("Failed to run herdr plugin pane open")?;

    // Parse response to extract pane_id for the lock file
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(pane_id) = extract_pane_id(&stdout) {
            log::info!("pane-open: opened pane {pane_id}");
            if let Err(e) = std::fs::write(&lock_path, &pane_id) {
                log::error!("Failed to write pane lock file: {e}");
            }
        }
    }

    std::process::exit(output.status.code().unwrap_or(1));
}

/// Extract pane_id from `herdr plugin pane open` JSON response.
///
/// Response shape:
///   {"id":"...","result":{"plugin_pane":{"pane":{"pane_id":"w1:p2",...}}}}
fn extract_pane_id(response: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(response).ok()?;
    v.get("result")?
        .get("plugin_pane")?
        .get("pane")?
        .get("pane_id")?
        .as_str()
        .map(String::from)
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::cli::Cli;

    /// Test that run_inner produces correct state with mock data.
    #[cfg(feature = "mock")]
    #[test]
    fn test_run_inner_with_mock_data() {
        let cli = Cli::parse_from(["herdr-recent-navigator", "--mock"]);
        let result = run_inner(&cli);
        assert!(result.is_ok(), "run_inner should succeed with --mock");

        let (state, pane_ts, tab_ts, ws_ts, ctx, connected) = result.unwrap();
        assert!(!state.nodes.is_empty(), "Mock data should produce nodes");
        assert!(!connected, "Mock mode should be disconnected");
        assert_eq!(state.current_category, CategoryTab::Workspaces);
        assert!(ctx.self_pane_id.is_none(), "No focused pane in mock mode");
        // Timestamp maps should be empty (no prior MRU data)
        assert!(pane_ts.is_empty());
        assert!(tab_ts.is_empty());
        assert!(ws_ts.is_empty());
    }

    /// Test that derive_active_context handles missing HERDR_PLUGIN_CONTEXT_JSON.
    #[test]
    fn test_derive_active_context_no_env() {
        let ctx = derive_active_context(&[], None);
        assert!(ctx.workspace_id.is_none());
        assert!(ctx.pane_id.is_none());
        assert!(ctx.tab_id.is_none());
        assert!(ctx.theme_name.is_none());
    }

    /// Verify that handle_track returns Ok when env is missing (realistic failure mode).
    /// This tests the error path — handle_track should return Err, not panic.
    #[test]
    fn test_handle_track_missing_env_returns_err() {
        // HERDR_PLUGIN_EVENT_JSON not set → should return Err
        let result = handle_track();
        assert!(
            result.is_err(),
            "handle_track should fail without HERDR_PLUGIN_EVENT_JSON"
        );
    }

    /// Verify extract_pane_id parses valid JSON.
    #[test]
    fn test_extract_pane_id_valid() {
        let json = r#"{"id":"1","result":{"plugin_pane":{"pane":{"pane_id":"w1:p2"}}}}"#;
        assert_eq!(extract_pane_id(json), Some("w1:p2".into()));
    }

    #[test]
    fn test_extract_pane_id_missing_fields() {
        let json = r#"{"id":"1"}"#;
        assert_eq!(extract_pane_id(json), None);
    }

    #[test]
    fn test_extract_pane_id_invalid_json() {
        assert_eq!(extract_pane_id("not json"), None);
    }
}
