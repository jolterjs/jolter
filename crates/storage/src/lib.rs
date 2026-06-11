use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use jolter_runtime::RuntimeKind;
use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Storage {
    root: PathBuf,
}

impl Storage {
    pub fn discover() -> Result<Self, StorageError> {
        if let Some(root) = env::var_os("JOLTER_HOME").filter(|value| !value.is_empty()) {
            return Ok(Self::new(root));
        }
        let home = home_directory().ok_or(StorageError::HomeDirectoryUnavailable)?;
        Ok(Self::new(home.join(".jolter")))
    }

    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn runtimes_dir(&self) -> PathBuf {
        self.root.join("runtimes")
    }

    #[must_use]
    pub fn runtime_dir(&self, kind: RuntimeKind) -> PathBuf {
        self.runtimes_dir().join(kind.to_string())
    }

    #[must_use]
    pub fn runtime_version_dir(&self, kind: RuntimeKind, version: &Version) -> PathBuf {
        self.runtime_dir(kind).join(version.to_string())
    }

    #[must_use]
    pub fn runtime_executable(&self, kind: RuntimeKind, version: &Version) -> PathBuf {
        let root = self.runtime_version_dir(kind, version);
        runtime_executable_in(&root, kind)
    }

    #[must_use]
    pub fn node_tool_executable(&self, version: &Version, tool: &str) -> PathBuf {
        let root = self.runtime_version_dir(RuntimeKind::Node, version);
        #[cfg(windows)]
        {
            root.join(format!("{tool}.cmd"))
        }
        #[cfg(not(windows))]
        {
            root.join("bin").join(tool)
        }
    }

    #[must_use]
    pub fn shims_dir(&self) -> PathBuf {
        self.root.join("shims")
    }

    #[must_use]
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    #[must_use]
    pub fn config_dir(&self) -> PathBuf {
        self.root.join("config")
    }

    pub fn ensure_layout(&self) -> Result<(), StorageError> {
        for path in [
            self.runtimes_dir(),
            self.shims_dir(),
            self.cache_dir(),
            self.config_dir(),
        ] {
            fs::create_dir_all(&path).map_err(|source| StorageError::Create {
                path: path.clone(),
                source,
            })?;
        }
        for kind in RuntimeKind::ALL {
            let path = self.runtime_dir(kind);
            fs::create_dir_all(&path).map_err(|source| StorageError::Create { path, source })?;
        }
        Ok(())
    }

    pub fn installed_runtimes(&self) -> Result<Vec<InstalledRuntime>, StorageError> {
        let mut installed = Vec::new();
        for kind in RuntimeKind::ALL {
            let directory = self.runtime_dir(kind);
            if !directory.exists() {
                continue;
            }
            let entries = fs::read_dir(&directory).map_err(|source| StorageError::Read {
                path: directory.clone(),
                source,
            })?;
            for entry in entries {
                let entry = entry.map_err(|source| StorageError::Read {
                    path: directory.clone(),
                    source,
                })?;
                if !entry
                    .file_type()
                    .map_err(|source| StorageError::Read {
                        path: entry.path(),
                        source,
                    })?
                    .is_dir()
                {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Ok(version) = Version::parse(name.trim_start_matches('v')) {
                    installed.push(InstalledRuntime {
                        kind,
                        version,
                        path: entry.path(),
                    });
                }
            }
        }
        installed.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.version.cmp(&right.version))
        });
        Ok(installed)
    }

    pub fn find_matching(
        &self,
        request: &jolter_runtime::RuntimeRequest,
    ) -> Result<Option<InstalledRuntime>, StorageError> {
        Ok(self
            .installed_runtimes()?
            .into_iter()
            .rev()
            .find(|runtime| {
                runtime.kind == request.kind
                    && request.matches_version(&runtime.version)
                    && runtime_executable_in(&runtime.path, runtime.kind).is_file()
            }))
    }

    pub fn activate(&self, kind: RuntimeKind, version: &Version) -> Result<(), StorageError> {
        let path = self.config_dir().join("active.json");
        let mut active = if path.is_file() {
            let contents = fs::read_to_string(&path).map_err(|source| StorageError::ReadFile {
                path: path.clone(),
                source,
            })?;
            serde_json::from_str::<ActiveRuntimes>(&contents).map_err(|source| {
                StorageError::ParseActive {
                    path: path.clone(),
                    source,
                }
            })?
        } else {
            ActiveRuntimes::default()
        };
        active
            .versions
            .insert(kind.to_string(), version.to_string());
        let contents = serde_json::to_vec_pretty(&active).map_err(StorageError::SerializeActive)?;
        atomic_write(&path, &contents)
    }

    pub fn active_version(&self, kind: RuntimeKind) -> Result<Option<Version>, StorageError> {
        let path = self.config_dir().join("active.json");
        if !path.is_file() {
            return Ok(None);
        }
        let contents = fs::read_to_string(&path).map_err(|source| StorageError::ReadFile {
            path: path.clone(),
            source,
        })?;
        let active: ActiveRuntimes =
            serde_json::from_str(&contents).map_err(|source| StorageError::ParseActive {
                path: path.clone(),
                source,
            })?;
        active
            .versions
            .get(&kind.to_string())
            .map(|value| {
                Version::parse(value).map_err(|source| StorageError::InvalidActiveVersion {
                    kind,
                    value: value.clone(),
                    source,
                })
            })
            .transpose()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledRuntime {
    pub kind: RuntimeKind,
    pub version: Version,
    pub path: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ActiveRuntimes {
    #[serde(flatten)]
    versions: BTreeMap<String, String>,
}

#[must_use]
pub fn runtime_executable_in(root: &Path, kind: RuntimeKind) -> PathBuf {
    match kind {
        RuntimeKind::Node => {
            #[cfg(windows)]
            {
                root.join("node.exe")
            }
            #[cfg(not(windows))]
            {
                root.join("bin").join("node")
            }
        }
        RuntimeKind::Bun => {
            #[cfg(windows)]
            {
                root.join("bun.exe")
            }
            #[cfg(not(windows))]
            {
                root.join("bun")
            }
        }
        RuntimeKind::Deno => {
            #[cfg(windows)]
            {
                root.join("deno.exe")
            }
            #[cfg(not(windows))]
            {
                root.join("deno")
            }
        }
    }
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), StorageError> {
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

fn home_directory() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("could not determine the user home directory; set JOLTER_HOME explicitly")]
    HomeDirectoryUnavailable,
    #[error("failed to create storage directory {path}: {source}")]
    Create {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read storage directory {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read storage file {path}: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write storage file {path}: {source}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid active runtime configuration at {path}: {source}")]
    ParseActive {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to serialize active runtime configuration: {0}")]
    SerializeActive(#[source] serde_json::Error),
    #[error("active {kind} version `{value}` is invalid: {source}")]
    InvalidActiveVersion {
        kind: RuntimeKind,
        value: String,
        #[source]
        source: semver::Error,
    },
    #[error("invalid storage path {path}")]
    InvalidPath { path: PathBuf },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_expected_layout() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();

        assert!(storage.runtime_dir(RuntimeKind::Node).is_dir());
        assert!(storage.runtime_dir(RuntimeKind::Bun).is_dir());
        assert!(storage.runtime_dir(RuntimeKind::Deno).is_dir());
        assert!(storage.shims_dir().is_dir());
    }

    #[test]
    fn lists_only_semver_runtime_directories() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();
        fs::create_dir(storage.runtime_dir(RuntimeKind::Node).join("24.1.0")).unwrap();
        fs::create_dir(
            storage
                .runtime_dir(RuntimeKind::Node)
                .join("partial-download"),
        )
        .unwrap();

        let installed = storage.installed_runtimes().unwrap();
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].version, Version::new(24, 1, 0));
    }

    #[test]
    fn persists_active_runtime_versions() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::new(temp.path());
        storage.ensure_layout().unwrap();

        storage
            .activate(RuntimeKind::Node, &Version::new(24, 2, 0))
            .unwrap();
        storage
            .activate(RuntimeKind::Node, &Version::new(24, 3, 0))
            .unwrap();

        assert_eq!(
            storage.active_version(RuntimeKind::Node).unwrap(),
            Some(Version::new(24, 3, 0))
        );
    }
}
