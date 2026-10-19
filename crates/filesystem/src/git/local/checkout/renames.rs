//! Bounded immutable-commit rename discovery shared by concurrent file reads.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::{CheckoutRuntimeError, SMALL_OUTPUT_LIMIT, paths, run_git};

type RenameMap = BTreeMap<String, String>;
type RenameResult = Result<Arc<RenameMap>, CheckoutRuntimeError>;

#[derive(Debug)]
struct Entry {
    created: Instant,
    result: Arc<OnceLock<RenameResult>>,
}

#[derive(Debug, Default)]
pub(super) struct RenameCache {
    entries: Mutex<BTreeMap<(PathBuf, String), Entry>>,
}

impl RenameCache {
    pub(super) fn read(&self, root: &Path, sha: &str) -> RenameResult {
        // Symbolic and abbreviated refs may identify a different commit on the next read.
        if !matches!(sha.len(), 40 | 64) || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return discover(root, sha);
        }
        let key = (root.to_path_buf(), sha.to_owned());
        let lookup = if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|_, entry| entry.created.elapsed() < Duration::from_secs(60));
            if entries.len() >= 100 && !entries.contains_key(&key) {
                entries.pop_first();
            }
            entries
                .entry(key.clone())
                .or_insert_with(|| Entry {
                    created: Instant::now(),
                    result: Arc::new(OnceLock::new()),
                })
                .result
                .clone()
        } else {
            return discover(root, sha);
        };
        let result = lookup.get_or_init(|| discover(root, sha)).clone();
        if result.is_err()
            && let Ok(mut entries) = self.entries.lock()
            && entries
                .get(&key)
                .is_some_and(|entry| Arc::ptr_eq(&entry.result, &lookup))
        {
            entries.remove(&key);
        }
        result
    }
}

fn discover(root: &Path, sha: &str) -> RenameResult {
    let output = run_git(
        root,
        &[
            "show",
            sha,
            "--format=",
            "--no-ext-diff",
            "--diff-merges=first-parent",
            "--name-status",
            "-z",
            "-M",
            "--diff-filter=R",
        ],
        &[0],
        SMALL_OUTPUT_LIMIT,
    )?
    .stdout;
    let mut fields = output.split('\0');
    let mut renames = BTreeMap::new();
    while let (Some(status), Some(old_path), Some(new_path)) =
        (fields.next(), fields.next(), fields.next())
    {
        if status.starts_with('R') {
            renames.insert(
                paths::decode(new_path).into_owned(),
                paths::decode(old_path).into_owned(),
            );
        }
    }
    Ok(Arc::new(renames))
}

#[cfg(test)]
mod tests;
