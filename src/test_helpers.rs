//! Shared test helpers for the entire crate.
//! Provides mock nodes, temp directory setup, and assertion helpers
//! used across all test modules.

use std::path::Path;
use std::sync::{Mutex, OnceLock};
use tempfile::TempDir;

static STATE_DIR_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

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
