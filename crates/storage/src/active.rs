use std::{collections::BTreeMap, fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::error::StorageError;

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct ActiveVersions {
    #[serde(flatten)]
    pub versions: BTreeMap<String, String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct ActivePluginTools {
    #[serde(default)]
    pub tools: BTreeMap<String, ActivePluginTool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ActivePluginTool {
    pub provider: String,
    pub version: String,
}

pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), StorageError> {
    let parent = path.parent().ok_or_else(|| StorageError::InvalidPath {
        path: path.to_path_buf(),
    })?;
    fs::create_dir_all(parent).map_err(|source| StorageError::Create {
        path: parent.to_path_buf(),
        source,
    })?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|source| StorageError::WriteFile {
            path: path.to_path_buf(),
            source,
        })?;
    std::io::Write::write_all(&mut temporary, contents).map_err(|source| {
        StorageError::WriteFile {
            path: path.to_path_buf(),
            source,
        }
    })?;
    temporary
        .persist(path)
        .map_err(|error| StorageError::WriteFile {
            path: path.to_path_buf(),
            source: error.error,
        })?;
    Ok(())
}
