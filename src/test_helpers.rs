//! Shared test helpers for the entire crate.
//! Provides mock nodes, temp directory setup, and assertion helpers
//! used across all test modules.

use std::path::Path;
use std::sync::{Mutex, OnceLock};
use tempfile::TempDir;

static STATE_DIR_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static CONFIG_PATH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static PLUGIN_DIRS_LOCK: Mutex<()> = Mutex::new(());

/// Create a temporary directory, set HERDR_PLUGIN_STATE_DIR to it,
/// call the closure, then clean up.
/// Uses a global lock to prevent concurrent env var conflicts in parallel tests.
pub fn with_temp_dir(f: impl FnOnce(&Path)) {
    let lock = STATE_DIR_LOCK.get_or_init(|| Mutex::new(()));
    // Use try_lock to avoid poisoning — if the lock is poisoned from a prior
    // panic, reset it.
    let _guard = match lock.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let dir = TempDir::new().expect("Failed to create temp dir");
    // SAFETY: We hold STATE_DIR_LOCK to prevent concurrent env var access,
    // which is the primary source of UB per Rust docs.
    unsafe {
        std::env::set_var("HERDR_PLUGIN_STATE_DIR", dir.path());
    }
    // Use catch_unwind to prevent mutex poisoning on test failure
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        f(dir.path());
    }));
    unsafe {
        std::env::remove_var("HERDR_PLUGIN_STATE_DIR");
    }
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

/// Write `contents` to a temporary Herdr config file, point
/// `HERDR_CONFIG_PATH` at it, call the closure, then clean up.
/// Pass `None` to leave the file absent while still overriding the env var.
/// Uses a global lock to prevent concurrent env var conflicts in parallel tests.
pub fn with_herdr_config(contents: Option<&str>, f: impl FnOnce()) {
    let lock = CONFIG_PATH_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = match lock.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("config.toml");
    if let Some(c) = contents {
        std::fs::write(&path, c).expect("Failed to write temp config");
    }
    // SAFETY: We hold CONFIG_PATH_LOCK to prevent concurrent env var access,
    // which is the primary source of UB per Rust docs.
    unsafe {
        std::env::set_var("HERDR_CONFIG_PATH", &path);
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    unsafe {
        std::env::remove_var("HERDR_CONFIG_PATH");
    }
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

/// Point `HERDR_PLUGIN_CONFIG_DIR` and `HERDR_PLUGIN_ROOT` at temp dirs holding
/// `config.toml` / `herdr-plugin.toml` with the given contents (`None` = absent),
/// call the closure, then clean up.
pub fn with_plugin_dirs(user_config: Option<&str>, manifest: Option<&str>, f: impl FnOnce()) {
    let _guard = match PLUGIN_DIRS_LOCK.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let config_dir = TempDir::new().expect("Failed to create temp dir");
    let root_dir = TempDir::new().expect("Failed to create temp dir");
    if let Some(c) = user_config {
        std::fs::write(config_dir.path().join("config.toml"), c).expect("write config");
    }
    if let Some(m) = manifest {
        std::fs::write(root_dir.path().join("herdr-plugin.toml"), m).expect("write manifest");
    }
    // SAFETY: We hold PLUGIN_DIRS_LOCK to prevent concurrent env var access.
    unsafe {
        std::env::set_var("HERDR_PLUGIN_CONFIG_DIR", config_dir.path());
        std::env::set_var("HERDR_PLUGIN_ROOT", root_dir.path());
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    unsafe {
        std::env::remove_var("HERDR_PLUGIN_CONFIG_DIR");
        std::env::remove_var("HERDR_PLUGIN_ROOT");
    }
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_with_temp_dir_sets_env_var() {
        with_temp_dir(|dir| {
            let env = std::env::var("HERDR_PLUGIN_STATE_DIR").unwrap();
            assert_eq!(PathBuf::from(env), dir);
        });
    }

    #[test]
    fn test_with_temp_dir_cleans_up_after() {
        let mut path = None;
        with_temp_dir(|dir| {
            path = Some(dir.to_path_buf());
            std::fs::write(dir.join("test.txt"), "hello").unwrap();
        });
        let path = path.unwrap();
        assert!(
            !path.exists(),
            "TempDir should be cleaned up after scope exit"
        );
    }
}
