use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;

use anyhow::Context;
use fs2::FileExt;
use serde::{Deserialize, Serialize};

/// The entity kind that was focused. Used to tag MRU entries so we can
/// apply pane-level, tab-level, and workspace-level timestamps independently.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum MruKind {
    Pane,
    Tab,
    Workspace,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MruEntry {
    pub kind: MruKind,
    /// The entity ID: pane_id, tab_id, or workspace_id depending on `kind`.
    pub id: String,
    /// The workspace that the focused entity belongs to.
    pub workspace_id: String,
    pub focused_at: u64,
    /// Human-readable name for the entity (tab label, pane label, workspace label).
    /// Used for debugging; populated when the caller has the data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Human-readable workspace label for context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_name: Option<String>,
}

/// Return the path to the state directory, or a default fallback.
pub fn state_dir_or_default() -> PathBuf {
    std::env::var("HERDR_PLUGIN_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/herdr-recent-navigator"))
}

/// Path to the MRU data file.
fn mru_path() -> PathBuf {
    state_dir_or_default().join("mru.json")
}

/// Path to the cross-process lock file.
fn lock_path() -> PathBuf {
    state_dir_or_default().join("mru.lock")
}

/// Acquire an exclusive lock on the lock file.
/// Blocks until the lock is acquired. Returns the locked file handle.
/// The lock is automatically released when the handle is dropped.
fn acquire_lock() -> anyhow::Result<File> {
    let path = lock_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .read(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("Failed to open lock file: {}", path.display()))?;
    file.lock_exclusive()?;
    Ok(file)
}

/// Record a focus event (pane/tab/workspace): persist a timestamped entry to the MRU file.
pub fn record_event(kind: MruKind, id: &str, workspace_id: &str) -> anyhow::Result<()> {
    record_event_with_names(kind, id, workspace_id, None, None)
}

/// Same as record_event but with human-readable names for logging/debugging.
///
/// Uses a cross-process lock file (`mru.lock`) to serialize concurrent writes
/// from multiple `track` subcommand processes that Herdr fires simultaneously.
pub fn record_event_with_names(
    kind: MruKind,
    id: &str,
    workspace_id: &str,
    name: Option<String>,
    workspace_name: Option<String>,
) -> anyhow::Result<()> {
    let path = mru_path();
    let _lock = acquire_lock()?;

    let now = chrono::Utc::now().timestamp_millis() as u64;

    let mut entries: Vec<MruEntry> = fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default();

    let old = entries
        .iter()
        .find(|e| e.kind == kind && e.id == id)
        .cloned();
    entries.retain(|e| !(e.kind == kind && e.id == id));

    entries.insert(
        0,
        MruEntry {
            kind,
            id: id.to_string(),
            workspace_id: workspace_id.to_string(),
            focused_at: now,
            name: name.or_else(|| old.as_ref().and_then(|o| o.name.clone())),
            workspace_name: workspace_name
                .or_else(|| old.as_ref().and_then(|o| o.workspace_name.clone())),
        },
    );

    entries.truncate(300);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Write to temp file then rename for atomicity.
    // If the process crashes mid-write, the original file is untouched.
    let tmp_path = path.with_extension("json.tmp");
    fs::write(&tmp_path, serde_json::to_string_pretty(&entries)?)?;
    fs::rename(&tmp_path, &path)?;

    Ok(())
    // Lock released when `_lock` is dropped
}

/// Load all MRU entries from the persistent state file.
pub fn load_mru() -> Vec<MruEntry> {
    let state_path = mru_path();
    let content = match std::fs::read_to_string(&state_path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    match serde_json::from_str(&content) {
        Ok(entries) => entries,
        Err(e) => {
            // Corrupted file: back up and return empty vec to avoid silent data loss
            log::error!("mru.json corrupted ({}), backing up to mru.json.bak", e);
            let bak_path = state_path.with_extension("json.bak");
            let _ = std::fs::rename(&state_path, &bak_path);
            vec![]
        }
    }
}

/// Build three separate timestamp maps (pane/tab/workspace) from MRU entries.
/// Each map returns the **most recent** focus timestamp for that entity.
///
/// These maps are passed to builders directly so each category tab
/// sorts using ONLY its own level's timestamps — no level mixing.
pub fn build_timestamp_maps(
    entries: &[MruEntry],
) -> (
    HashMap<String, u64>,
    HashMap<String, u64>,
    HashMap<String, u64>,
) {
    let mut pane_ts: HashMap<String, u64> = HashMap::new();
    let mut tab_ts: HashMap<String, u64> = HashMap::new();
    let mut ws_ts: HashMap<String, u64> = HashMap::new();

    for entry in entries {
        let ts = entry.focused_at;
        match entry.kind {
            MruKind::Pane => {
                pane_ts.entry(entry.id.clone()).or_insert(ts);
            }
            MruKind::Tab => {
                tab_ts.entry(entry.id.clone()).or_insert(ts);
            }
            MruKind::Workspace => {
                ws_ts.entry(entry.id.clone()).or_insert(ts);
            }
        }
    }

    (pane_ts, tab_ts, ws_ts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::with_temp_dir;

    // ── record_event tests ──

    #[test]
    fn test_record_event_creates_file() {
        with_temp_dir(|dir| {
            let path = dir.join("mru.json");
            assert!(!path.exists(), "File should not exist before recording");
            record_event(MruKind::Pane, "pane-1", "ws-1").unwrap();
            assert!(path.exists(), "File should exist after recording");
        });
    }

    #[test]
    fn test_record_event_updates_timestamp() {
        with_temp_dir(|_dir| {
            record_event(MruKind::Pane, "pane-1", "ws-1").unwrap();
            let entries = load_mru();
            assert_eq!(entries.len(), 1);
            let t1 = entries[0].focused_at;

            std::thread::sleep(std::time::Duration::from_millis(1));
            record_event(MruKind::Pane, "pane-1", "ws-1").unwrap();
            let entries = load_mru();
            assert_eq!(entries.len(), 1, "Duplicate should not add new entry");
            assert!(entries[0].focused_at > t1, "Timestamp should be updated");
        });
    }

    #[test]
    fn test_record_event_truncates_at_300() {
        with_temp_dir(|_dir| {
            for i in 0..301 {
                record_event(MruKind::Pane, &format!("pane-{}", i), "ws-1").unwrap();
            }
            let entries = load_mru();
            assert_eq!(entries.len(), 300, "Should truncate to 300 entries");
            assert_eq!(entries[0].id, "pane-300", "Most recent should be first");
        });
    }

    #[test]
    fn test_record_event_preserves_name_on_update() {
        with_temp_dir(|_dir| {
            record_event_with_names(MruKind::Pane, "pane-1", "ws-1", Some("MyPane".into()), None)
                .unwrap();

            record_event_with_names(MruKind::Pane, "pane-1", "ws-1", None, None).unwrap();

            let entries = load_mru();
            assert_eq!(
                entries[0].name.as_deref(),
                Some("MyPane"),
                "Name should be preserved from previous entry"
            );
        });
    }

    // ── load_mru tests ──

    #[test]
    fn test_load_mru_returns_empty_on_missing_file() {
        with_temp_dir(|_dir| {
            let entries = load_mru();
            assert!(entries.is_empty(), "No file should return empty vec");
        });
    }

    #[test]
    fn test_load_mru_returns_empty_on_corrupt_file() {
        with_temp_dir(|dir| {
            let path = dir.join("mru.json");
            std::fs::write(&path, "{{{corrupted data}}}").unwrap();
            let entries = load_mru();
            assert!(entries.is_empty(), "Corrupt file should return empty vec");
            assert!(
                path.with_extension("json.bak").exists(),
                "Backup should be created for corrupt file"
            );
        });
    }

    #[test]
    fn test_load_mru_reads_valid_entries() {
        with_temp_dir(|dir| {
            let path = dir.join("mru.json");
            let data = r#"[
                {"kind":"Pane","id":"pane-1","workspace_id":"ws-1","focused_at":1000}
            ]"#;
            std::fs::write(&path, data).unwrap();
            let entries = load_mru();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].id, "pane-1");
        });
    }

    // ── build_timestamp_maps tests ──

    #[test]
    fn test_build_timestamp_maps_separates_kinds() {
        let entries = vec![
            MruEntry {
                kind: MruKind::Pane,
                id: "same-id".into(),
                workspace_id: "ws".into(),
                focused_at: 100,
                name: None,
                workspace_name: None,
            },
            MruEntry {
                kind: MruKind::Tab,
                id: "same-id".into(),
                workspace_id: "ws".into(),
                focused_at: 200,
                name: None,
                workspace_name: None,
            },
            MruEntry {
                kind: MruKind::Workspace,
                id: "same-id".into(),
                workspace_id: "ws".into(),
                focused_at: 300,
                name: None,
                workspace_name: None,
            },
        ];
        let (pane, tab, ws) = build_timestamp_maps(&entries);
        assert_eq!(pane.get("same-id"), Some(&100));
        assert_eq!(tab.get("same-id"), Some(&200));
        assert_eq!(ws.get("same-id"), Some(&300));
    }

    #[test]
    fn test_build_timestamp_maps_keeps_most_recent() {
        // MRU order: most recent first (999 before 100)
        let entries = vec![
            MruEntry {
                kind: MruKind::Pane,
                id: "pane-1".into(),
                workspace_id: "ws".into(),
                focused_at: 999,
                name: None,
                workspace_name: None,
            },
            MruEntry {
                kind: MruKind::Pane,
                id: "pane-1".into(),
                workspace_id: "ws".into(),
                focused_at: 100,
                name: None,
                workspace_name: None,
            },
        ];
        let (pane, _, _) = build_timestamp_maps(&entries);
        assert_eq!(
            pane.get("pane-1"),
            Some(&999),
            "Should keep most recent (first seen in MRU order)"
        );
    }

    // ── atomic write test ──

    #[test]
    fn test_record_event_atomic_write_survives_crash() {
        with_temp_dir(|dir| {
            record_event(MruKind::Pane, "pane-1", "ws-1").unwrap();
            let path = dir.join("mru.json");
            let content_before = std::fs::read_to_string(&path).unwrap();

            // Simulate partial write: tmp file exists but rename never happened
            let tmp_path = path.with_extension("json.tmp");
            std::fs::write(&tmp_path, "{{{partial}}}").unwrap();
            // Don't rename — simulates crash

            let content_after = std::fs::read_to_string(&path).unwrap();
            assert_eq!(
                content_before, content_after,
                "Crash during write should not corrupt original file"
            );
        });
    }
}
