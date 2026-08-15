use std::{fs, path::Path};

use crate::{error::StorageError, types::CacheStats};

pub(crate) fn directory_stats(path: &Path) -> Result<CacheStats, StorageError> {
    if !path.exists() {
        return Ok(CacheStats::default());
    }
    let metadata = fs::symlink_metadata(path).map_err(|source| StorageError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        return Ok(CacheStats {
            files: 1,
            bytes: metadata.len(),
        });
    }

    let mut stats = CacheStats::default();
    let entries = fs::read_dir(path).map_err(|source| StorageError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| StorageError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let child = directory_stats(&entry.path())?;
        stats.files = stats.files.saturating_add(child.files);
        stats.bytes = stats.bytes.saturating_add(child.bytes);
    }
    Ok(stats)
}
