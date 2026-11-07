// File: src/app/cache.rs
// Purpose: GPU texture and thumbnail cache key migration and remapping across directory sort updates.

use std::collections::HashMap;
use std::path::PathBuf;

/// Move entries from `cache` to their new indices according to file path matches,
/// optionally remapping an explicitly renamed item (`(old_idx, new_idx)`).
pub(crate) fn remap_cache_keys_with_rename<T>(
    cache: &mut HashMap<usize, T>,
    old_files: &[PathBuf],
    new_files: &[PathBuf],
    renamed: Option<(usize, usize)>,
) -> HashMap<usize, T> {
    let mut new_cache = HashMap::new();
    for (new_idx, new_path) in new_files.iter().enumerate() {
        if let Some((old_renamed_idx, new_renamed_idx)) = renamed {
            if new_idx == new_renamed_idx {
                if let Some(val) = cache.remove(&old_renamed_idx) {
                    new_cache.insert(new_idx, val);
                    continue;
                }
            }
        }
        if let Some(old_idx) = old_files.iter().position(|p| p == new_path) {
            if let Some(val) = cache.remove(&old_idx) {
                new_cache.insert(new_idx, val);
            }
        }
    }
    new_cache
}

/// Move entries from `cache` to their new indices according to file path matches.
pub(crate) fn remap_cache_keys<T>(
    cache: &mut HashMap<usize, T>,
    old_files: &[PathBuf],
    new_files: &[PathBuf],
) -> HashMap<usize, T> {
    remap_cache_keys_with_rename(cache, old_files, new_files, None)
}
