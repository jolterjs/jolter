use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

use fs4::FileExt;
use jolter_runtime::{RuntimeKind, RuntimeRequest, ToolKind, ToolRequest};
use jolter_storage::{CacheStats, InstalledRuntime, InstalledTool, Storage, runtime_executable_in};
use semver::Version;
use sha2::{Digest, Sha256};

use crate::{
    MAX_METADATA_CACHE_AGE_SECS,
    archive::{extract_archive, validate_node_engine},
    error::InstallerError,
    http::{
        HttpClient, ReqwestHttpClient, ensure_https, offline_mode, verify_tool_hash,
        write_cache_file,
    },
    progress::{NoProgressReporter, ProgressAction, ProgressEvent, ProgressReporter},
    providers::{github::GithubRuntime, npm::InstalledPackageMetadata},
    types::{
        ArchiveFormat, Artifact, ArtifactIntegrity, CacheCleanOutcome, InstallOutcome,
        OperatingSystem, Platform, PluginToolArchive, PluginToolInstallOutcome, Release,
        ReleaseChannel, RemovalOutcome, SelfUpgradeOutcome, ToolInstallOutcome,
    },
};

pub mod manifests;

pub use manifests::{write_manifest, write_plugin_tool_manifest, write_tool_manifest};

pub struct Installer {
    pub(crate) storage: Storage,
    pub(crate) platform: Platform,
    pub(crate) http: HttpClientSource,
    pub(crate) reporter: Arc<dyn ProgressReporter>,
}

pub(crate) enum HttpClientSource {
    Lazy(Mutex<Option<Arc<dyn HttpClient>>>),
    Ready(Arc<dyn HttpClient>),
}

impl HttpClientSource {
    pub(crate) fn get(&self) -> Result<Arc<dyn HttpClient>, InstallerError> {
        match self {
            Self::Lazy(client) => {
                let mut client = client.lock().unwrap();
                if client.is_none() {
                    *client = Some(Arc::new(ReqwestHttpClient::new()?));
                }
                Ok(client.as_ref().expect("client initialized above").clone())
            }
            Self::Ready(client) => Ok(client.clone()),
        }
    }
}

impl Installer {
    pub fn new(storage: Storage) -> Result<Self, InstallerError> {
        Self::new_with_reporter(storage, Arc::new(NoProgressReporter))
    }

    pub fn new_with_reporter(
        storage: Storage,
        reporter: Arc<dyn ProgressReporter>,
    ) -> Result<Self, InstallerError> {
        Ok(Self {
            storage,
            platform: Platform::current()?,
            http: HttpClientSource::Lazy(Mutex::new(None)),
            reporter,
        })
    }

    #[must_use]
    pub fn with_client(storage: Storage, platform: Platform, http: Arc<dyn HttpClient>) -> Self {
        Self::with_client_and_reporter(storage, platform, http, Arc::new(NoProgressReporter))
    }

    #[must_use]
    pub fn with_client_and_reporter(
        storage: Storage,
        platform: Platform,
        http: Arc<dyn HttpClient>,
        reporter: Arc<dyn ProgressReporter>,
    ) -> Self {
        Self {
            storage,
            platform,
            http: HttpClientSource::Ready(http),
            reporter,
        }
    }

    pub fn resolve(&self, request: &RuntimeRequest) -> Result<Release, InstallerError> {
        match request.kind {
            RuntimeKind::Node => self.resolve_node(request),
            RuntimeKind::Bun => self.resolve_github(request, GithubRuntime::Bun),
            RuntimeKind::Deno => self.resolve_github(request, GithubRuntime::Deno),
        }
    }

    pub fn install(&self, request: &RuntimeRequest) -> Result<InstallOutcome, InstallerError> {
        self.install_inner(request, false)
    }

    pub fn repair(&self, request: &RuntimeRequest) -> Result<InstallOutcome, InstallerError> {
        self.install_inner(request, true)
    }

    pub fn install_tool(
        &self,
        request: &ToolRequest,
        node_version: &Version,
    ) -> Result<ToolInstallOutcome, InstallerError> {
        self.install_tool_inner(request, node_version, false)
    }

    pub fn repair_tool(
        &self,
        request: &ToolRequest,
        node_version: &Version,
    ) -> Result<ToolInstallOutcome, InstallerError> {
        self.install_tool_inner(request, node_version, true)
    }

    pub fn install_plugin_tool(
        &self,
        release: &PluginToolArchive,
        repair: bool,
    ) -> Result<PluginToolInstallOutcome, InstallerError> {
        self.storage.ensure_layout()?;
        let maintenance = self.maintenance_lock()?;
        FileExt::lock_shared(&maintenance).map_err(InstallerError::Io)?;
        let target = format!(
            "{}@{} via {}",
            release.tool, release.version, release.provider
        );
        self.report_stage(ProgressAction::Resolve, &target);
        let destination = self.storage.plugin_tool_version_dir(
            &release.provider,
            &release.tool,
            &release.version,
        );
        let lock =
            self.plugin_tool_install_lock(&release.provider, &release.tool, &release.version)?;
        FileExt::lock(&lock).map_err(InstallerError::Io)?;

        if plugin_tool_commands_exist(&destination, &release.commands) {
            self.report_stage(ProgressAction::Reuse, &target);
            return Ok(PluginToolInstallOutcome {
                provider: release.provider.clone(),
                tool: release.tool.clone(),
                version: release.version.clone(),
                path: destination,
                commands: release.commands.clone(),
                downloaded: false,
            });
        }
        if destination.exists() {
            if !repair {
                return Err(InstallerError::CorruptPluginToolInstallation { path: destination });
            }
            let expected_parent = self
                .storage
                .plugin_tool_dir(&release.provider, &release.tool);
            if destination.parent() != Some(expected_parent.as_path()) {
                return Err(InstallerError::UnsafeRemoval { path: destination });
            }
            fs::remove_dir_all(&destination).map_err(|source| InstallerError::RemoveCorrupt {
                path: destination.clone(),
                source,
            })?;
        }

        release.artifact.validate()?;
        let archive = self.obtain_archive(&release.artifact)?;
        let tool_parent = self
            .storage
            .plugin_tool_dir(&release.provider, &release.tool);
        fs::create_dir_all(&tool_parent).map_err(InstallerError::Io)?;
        let stage = tempfile::Builder::new()
            .prefix(".jolter-plugin-tool-install-")
            .tempdir_in(&tool_parent)
            .map_err(InstallerError::Io)?;
        let payload = stage.path().join("payload");
        fs::create_dir(&payload).map_err(InstallerError::Io)?;
        self.report_stage(ProgressAction::Extract, &target);
        extract_archive(
            &archive,
            &payload,
            release.artifact.format,
            release.artifact.strip_components,
        )?;

        for command in &release.commands {
            let executable = plugin_tool_executable(&payload, command);
            if !executable.is_file() {
                return Err(InstallerError::ExecutableMissing { path: executable });
            }
            make_executable(&executable)?;
        }
        write_plugin_tool_manifest(&payload, release)?;
        self.report_stage(ProgressAction::Publish, &target);
        fs::rename(&payload, &destination).map_err(|source| InstallerError::Publish {
            path: destination.clone(),
            source,
        })?;

        Ok(PluginToolInstallOutcome {
            provider: release.provider.clone(),
            tool: release.tool.clone(),
            version: release.version.clone(),
            path: destination,
            commands: release.commands.clone(),
            downloaded: true,
        })
    }

    pub fn validate_installed_tool(
        &self,
        tool: &InstalledTool,
        node_version: &Version,
    ) -> Result<(), InstallerError> {
        let path = tool.path.join("package.json");
        if !path.is_file() {
            return Ok(());
        }
        let contents = fs::read_to_string(&path).map_err(|source| {
            InstallerError::InstalledPackageMetadataRead {
                path: path.clone(),
                source,
            }
        })?;
        let metadata: InstalledPackageMetadata =
            serde_json::from_str(&contents).map_err(|source| {
                InstallerError::InstalledPackageMetadataParse {
                    path: path.clone(),
                    source,
                }
            })?;
        validate_node_engine(
            tool.kind,
            &tool.version,
            metadata.engines.node.as_deref(),
            node_version,
        )
    }

    pub fn resolve_self_release(
        &self,
        channel: ReleaseChannel,
    ) -> Result<(Version, Artifact), InstallerError> {
        let (os_name, arch_name) = match (self.platform.os, self.platform.arch) {
            (OperatingSystem::Windows, crate::types::Architecture::X64) => {
                ("pc-windows-msvc", "x86_64")
            }
            (OperatingSystem::Windows, crate::types::Architecture::Arm64) => {
                ("pc-windows-msvc", "aarch64")
            }
            (OperatingSystem::Linux, crate::types::Architecture::X64) => {
                ("unknown-linux-gnu", "x86_64")
            }
            (OperatingSystem::Linux, crate::types::Architecture::Arm64) => {
                ("unknown-linux-gnu", "aarch64")
            }
            (OperatingSystem::MacOs, crate::types::Architecture::X64) => ("apple-darwin", "x86_64"),
            (OperatingSystem::MacOs, crate::types::Architecture::Arm64) => {
                ("apple-darwin", "aarch64")
            }
        };
        let target_triple = format!("{arch_name}-{os_name}");
        let format = if self.platform.os == OperatingSystem::Windows {
            ArchiveFormat::Zip
        } else {
            ArchiveFormat::TarGz
        };
        let archive_ext = format.cache_extension();

        let (tag_name, release_info_url) = match channel {
            ReleaseChannel::Stable => (
                "latest".to_owned(),
                "https://api.github.com/repos/jolterjs/jolter/releases/latest".to_owned(),
            ),
            ReleaseChannel::Nightly => (
                "nightly".to_owned(),
                "https://api.github.com/repos/jolterjs/jolter/releases".to_owned(),
            ),
        };

        let raw_text = self.http.get()?.get_text(&release_info_url)?;
        let parsed: serde_json::Value =
            serde_json::from_str(&raw_text).map_err(|source| InstallerError::MetadataJson {
                url: release_info_url.clone(),
                source,
            })?;

        let resolved_tag = match &parsed {
            serde_json::Value::Array(releases) => releases
                .iter()
                .find(|r| {
                    let draft = r
                        .get("draft")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let tag = r
                        .get("tag_name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    !draft && (tag.contains("nightly") || tag.contains("-nightly."))
                })
                .and_then(|r| r.get("tag_name"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&tag_name)
                .to_owned(),
            _ => parsed
                .get("tag_name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&tag_name)
                .to_owned(),
        };

        let clean_version_str = resolved_tag.trim_start_matches('v');
        let version = Version::parse(clean_version_str).unwrap_or_else(|_| Version::new(0, 0, 0));

        let archive_name = format!("jolter-{resolved_tag}-{target_triple}.{archive_ext}");
        let download_url = format!(
            "https://github.com/jolterjs/jolter/releases/download/{resolved_tag}/{archive_name}"
        );
        let checksum_url = format!("{download_url}.sha256");

        let checksum_text = self.http.get()?.get_text(&checksum_url)?;
        let sha256_val = crate::http::parse_checksum_value(&checksum_text)?;

        Ok((
            version,
            Artifact {
                url: download_url,
                integrity: ArtifactIntegrity::Sha256(sha256_val),
                file_name: archive_name,
                format,
                strip_components: 1,
            },
        ))
    }

    pub fn upgrade_self(
        &self,
        channel: ReleaseChannel,
        force: bool,
    ) -> Result<SelfUpgradeOutcome, InstallerError> {
        let current_ver =
            Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0));

        let (version, artifact) = self.resolve_self_release(channel)?;

        let should_skip = if force {
            false
        } else {
            match channel {
                ReleaseChannel::Stable => version <= current_ver,
                ReleaseChannel::Nightly => version == current_ver,
            }
        };

        if should_skip {
            let target_executable =
                std::env::current_exe().unwrap_or_else(|_| self.storage.shims_dir().join("jolter"));
            return Ok(SelfUpgradeOutcome {
                channel,
                version,
                updated: false,
                executable_path: target_executable,
            });
        }

        let target_label = format!("jolter@{version} ({channel})");
        self.report_stage(ProgressAction::Resolve, &target_label);

        artifact.validate()?;
        let archive_path = self.obtain_archive(&artifact)?;

        let temp_dir = tempfile::Builder::new()
            .prefix(".jolter-upgrade-")
            .tempdir_in(self.storage.root())
            .map_err(InstallerError::Io)?;

        self.report_stage(ProgressAction::Extract, &target_label);
        extract_archive(
            &archive_path,
            temp_dir.path(),
            artifact.format,
            artifact.strip_components,
        )?;

        let exe_name = if cfg!(windows) {
            "jolter.exe"
        } else {
            "jolter"
        };
        let new_binary = temp_dir.path().join(exe_name);
        if !new_binary.is_file() {
            return Err(InstallerError::ExecutableMissing { path: new_binary });
        }
        make_executable(&new_binary)?;

        let target_dir = self.storage.root().join("bin");
        fs::create_dir_all(&target_dir).map_err(InstallerError::Io)?;
        let target_executable = target_dir.join(exe_name);

        self.report_stage(ProgressAction::Publish, &target_label);

        #[cfg(windows)]
        {
            if target_executable.exists() {
                let old_executable =
                    target_dir.join(format!("{exe_name}.old.{}", std::process::id()));
                let _ = fs::rename(&target_executable, &old_executable);
            }
            fs::copy(&new_binary, &target_executable).map_err(|source| {
                InstallerError::Publish {
                    path: target_executable.clone(),
                    source,
                }
            })?;
        }

        #[cfg(not(windows))]
        {
            let temp_dest = target_dir.join(format!(".{exe_name}.tmp.{}", std::process::id()));
            fs::copy(&new_binary, &temp_dest).map_err(|source| InstallerError::Publish {
                path: temp_dest.clone(),
                source,
            })?;
            make_executable(&temp_dest)?;
            fs::rename(&temp_dest, &target_executable).map_err(|source| {
                InstallerError::Publish {
                    path: target_executable.clone(),
                    source,
                }
            })?;
        }

        Ok(SelfUpgradeOutcome {
            channel,
            version,
            updated: true,
            executable_path: target_executable,
        })
    }

    pub fn uninstall_runtime(
        &self,
        kind: RuntimeKind,
        version: &Version,
    ) -> Result<RemovalOutcome, InstallerError> {
        self.storage.ensure_layout()?;
        let maintenance = self.maintenance_lock()?;
        FileExt::lock_shared(&maintenance).map_err(InstallerError::Io)?;
        let install = self.install_lock(kind, version)?;
        FileExt::lock(&install).map_err(InstallerError::Io)?;
        let path = self.storage.runtime_version_dir(kind, version);
        remove_installation(
            &self.storage,
            path,
            &self.storage.runtime_dir(kind),
            InstallationType::Runtime,
        )
    }

    pub fn uninstall_tool(
        &self,
        kind: ToolKind,
        version: &Version,
    ) -> Result<RemovalOutcome, InstallerError> {
        self.storage.ensure_layout()?;
        let maintenance = self.maintenance_lock()?;
        FileExt::lock_shared(&maintenance).map_err(InstallerError::Io)?;
        let install = self.tool_install_lock(kind, version)?;
        FileExt::lock(&install).map_err(InstallerError::Io)?;
        let path = self.storage.tool_version_dir(kind, version);
        remove_installation(
            &self.storage,
            path,
            &self.storage.tool_dir(kind),
            InstallationType::Tool,
        )
    }

    pub fn uninstall_plugin_tool(
        &self,
        provider: &str,
        tool: &str,
        version: &Version,
    ) -> Result<RemovalOutcome, InstallerError> {
        self.storage.ensure_layout()?;
        let maintenance = self.maintenance_lock()?;
        FileExt::lock_shared(&maintenance).map_err(InstallerError::Io)?;
        let install = self.plugin_tool_install_lock(provider, tool, version)?;
        FileExt::lock(&install).map_err(InstallerError::Io)?;
        let path = self
            .storage
            .plugin_tool_version_dir(provider, tool, version);
        remove_installation(
            &self.storage,
            path,
            &self.storage.plugin_tool_dir(provider, tool),
            InstallationType::Tool,
        )
    }

    pub fn clean_cache(&self) -> Result<CacheCleanOutcome, InstallerError> {
        self.storage.ensure_layout()?;
        let maintenance = self.maintenance_lock()?;
        FileExt::lock(&maintenance).map_err(InstallerError::Io)?;
        let mut outcome = CacheCleanOutcome {
            reclaimed_bytes: 0,
            removed_files: 0,
        };
        for name in ["downloads", "metadata"] {
            let path = self.storage.cache_dir().join(name);
            let stats = self.storage.path_stats(&path)?;
            let removed_files = usize::try_from(stats.files).unwrap_or(usize::MAX);
            outcome.removed_files = outcome.removed_files.saturating_add(removed_files);
            outcome.reclaimed_bytes = outcome.reclaimed_bytes.saturating_add(stats.bytes);
            if path.exists() {
                fs::remove_dir_all(&path).map_err(|source| InstallerError::CacheCleanup {
                    path: path.clone(),
                    source,
                })?;
            }
            fs::create_dir_all(&path).map_err(InstallerError::Io)?;
        }
        Ok(outcome)
    }

    fn install_tool_inner(
        &self,
        request: &ToolRequest,
        node_version: &Version,
        repair: bool,
    ) -> Result<ToolInstallOutcome, InstallerError> {
        self.storage.ensure_layout()?;
        let maintenance = self.maintenance_lock()?;
        FileExt::lock_shared(&maintenance).map_err(InstallerError::Io)?;
        let requested = request.to_string();
        self.report_stage(ProgressAction::Resolve, &requested);
        let release = self.resolve_tool(request)?;
        let target = format!("{}@{}", release.kind, release.version);
        validate_node_engine(
            release.kind,
            &release.version,
            release.node_engine.as_deref(),
            node_version,
        )?;
        let destination = self
            .storage
            .tool_version_dir(release.kind, &release.version);
        let lock = self.tool_install_lock(release.kind, &release.version)?;
        FileExt::lock(&lock).map_err(InstallerError::Io)?;

        let command = release.kind.to_string();
        let entrypoint = release
            .kind
            .entrypoint(&command)
            .ok_or(InstallerError::MissingToolEntrypoint(release.kind))?;
        let executable = destination.join(entrypoint);
        let verified_archive = if let Some(hash) = &release.expected_hash {
            release.artifact.validate()?;
            let archive = self.obtain_archive(&release.artifact)?;
            self.report_stage(ProgressAction::Verify, &release.artifact.file_name);
            verify_tool_hash(&archive, hash)?;
            Some(archive)
        } else {
            None
        };
        if executable.is_file() {
            self.report_stage(ProgressAction::Reuse, &target);
            return Ok(ToolInstallOutcome {
                tool: InstalledTool {
                    kind: release.kind,
                    version: release.version,
                    path: destination,
                },
                downloaded: false,
            });
        }
        if destination.exists() {
            if !repair {
                return Err(InstallerError::CorruptToolInstallation { path: destination });
            }
            let expected_parent = self.storage.tool_dir(release.kind);
            if destination.parent() != Some(expected_parent.as_path()) {
                return Err(InstallerError::UnsafeRemoval { path: destination });
            }
            fs::remove_dir_all(&destination).map_err(|source| InstallerError::RemoveCorrupt {
                path: destination.clone(),
                source,
            })?;
        }

        let archive = if let Some(archive) = verified_archive {
            archive
        } else {
            release.artifact.validate()?;
            self.obtain_archive(&release.artifact)?
        };
        let tool_parent = self.storage.tool_dir(release.kind);
        let stage = tempfile::Builder::new()
            .prefix(".jolter-tool-install-")
            .tempdir_in(&tool_parent)
            .map_err(InstallerError::Io)?;
        let payload = stage.path().join("payload");
        fs::create_dir(&payload).map_err(InstallerError::Io)?;
        self.report_stage(ProgressAction::Extract, &target);
        extract_archive(
            &archive,
            &payload,
            release.artifact.format,
            release.artifact.strip_components,
        )?;

        let staged_entrypoint = payload.join(entrypoint);
        if !staged_entrypoint.is_file() {
            return Err(InstallerError::ExecutableMissing {
                path: staged_entrypoint,
            });
        }
        make_executable(&staged_entrypoint)?;
        write_tool_manifest(&payload, &release)?;
        self.report_stage(ProgressAction::Publish, &target);
        fs::rename(&payload, &destination).map_err(|source| InstallerError::Publish {
            path: destination.clone(),
            source,
        })?;

        Ok(ToolInstallOutcome {
            tool: InstalledTool {
                kind: release.kind,
                version: release.version,
                path: destination,
            },
            downloaded: true,
        })
    }

    fn install_inner(
        &self,
        request: &RuntimeRequest,
        repair: bool,
    ) -> Result<InstallOutcome, InstallerError> {
        self.storage.ensure_layout()?;
        let maintenance = self.maintenance_lock()?;
        FileExt::lock_shared(&maintenance).map_err(InstallerError::Io)?;
        let requested = request.to_string();
        self.report_stage(ProgressAction::Resolve, &requested);
        let release = self.resolve(request)?;
        let target = format!("{}@{}", release.kind, release.version);
        let destination = self
            .storage
            .runtime_version_dir(release.kind, &release.version);
        let lock = self.install_lock(release.kind, &release.version)?;
        FileExt::lock(&lock).map_err(InstallerError::Io)?;

        let executable = runtime_executable_in(&destination, release.kind);
        if executable.is_file() {
            self.report_stage(ProgressAction::Reuse, &target);
            return Ok(InstallOutcome {
                runtime: InstalledRuntime {
                    kind: release.kind,
                    version: release.version,
                    path: destination,
                },
                downloaded: false,
            });
        }
        if destination.exists() {
            if !repair {
                return Err(InstallerError::CorruptInstallation { path: destination });
            }
            let expected_parent = self.storage.runtime_dir(release.kind);
            if destination.parent() != Some(expected_parent.as_path()) {
                return Err(InstallerError::UnsafeRemoval { path: destination });
            }
            fs::remove_dir_all(&destination).map_err(|source| InstallerError::RemoveCorrupt {
                path: destination.clone(),
                source,
            })?;
        }

        release.artifact.validate()?;
        let archive = self.obtain_archive(&release.artifact)?;
        let runtime_parent = self.storage.runtime_dir(release.kind);
        let stage = tempfile::Builder::new()
            .prefix(".jolter-install-")
            .tempdir_in(&runtime_parent)
            .map_err(InstallerError::Io)?;
        let payload = stage.path().join("payload");
        fs::create_dir(&payload).map_err(InstallerError::Io)?;
        self.report_stage(ProgressAction::Extract, &target);
        extract_archive(
            &archive,
            &payload,
            release.artifact.format,
            release.artifact.strip_components,
        )?;

        let staged_executable = runtime_executable_in(&payload, release.kind);
        if !staged_executable.is_file() {
            return Err(InstallerError::ExecutableMissing {
                path: staged_executable,
            });
        }
        make_executable(&staged_executable)?;
        write_manifest(&payload, &release)?;
        self.report_stage(ProgressAction::Publish, &target);
        fs::rename(&payload, &destination).map_err(|source| InstallerError::Publish {
            path: destination.clone(),
            source,
        })?;

        Ok(InstallOutcome {
            runtime: InstalledRuntime {
                kind: release.kind,
                version: release.version,
                path: destination,
            },
            downloaded: true,
        })
    }

    fn install_lock(&self, kind: RuntimeKind, version: &Version) -> Result<File, InstallerError> {
        let directory = self.storage.cache_dir().join("locks");
        fs::create_dir_all(&directory).map_err(InstallerError::Io)?;
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join(format!("{kind}-{version}.lock")))
            .map_err(InstallerError::Io)
    }

    fn tool_install_lock(&self, kind: ToolKind, version: &Version) -> Result<File, InstallerError> {
        let directory = self.storage.cache_dir().join("locks");
        fs::create_dir_all(&directory).map_err(InstallerError::Io)?;
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join(format!("tool-{kind}-{version}.lock")))
            .map_err(InstallerError::Io)
    }

    fn plugin_tool_install_lock(
        &self,
        provider: &str,
        tool: &str,
        version: &Version,
    ) -> Result<File, InstallerError> {
        let safe_provider = provider.replace(['@', '/'], "_");
        let path = self
            .storage
            .cache_dir()
            .join("locks")
            .join(format!("plugin-tool-{safe_provider}-{tool}-{version}.lock"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(InstallerError::Io)?;
        }
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(InstallerError::Io)
    }

    fn maintenance_lock(&self) -> Result<File, InstallerError> {
        let directory = self.storage.cache_dir().join("locks");
        fs::create_dir_all(&directory).map_err(InstallerError::Io)?;
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join("maintenance.lock"))
            .map_err(InstallerError::Io)
    }

    fn metadata_lock(&self, cache_key: &str) -> Result<File, InstallerError> {
        let directory = self.storage.cache_dir().join("locks");
        fs::create_dir_all(&directory).map_err(InstallerError::Io)?;
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join(format!("metadata-{cache_key}.lock")))
            .map_err(InstallerError::Io)
    }

    pub(crate) fn obtain_archive(&self, artifact: &Artifact) -> Result<PathBuf, InstallerError> {
        let directory = self.storage.cache_dir().join("downloads");
        fs::create_dir_all(&directory).map_err(InstallerError::Io)?;
        let path = directory.join(format!(
            "{}.{}",
            artifact.integrity.cache_key(),
            artifact.format.cache_extension()
        ));
        if path.is_file() {
            self.report_stage(ProgressAction::Verify, &artifact.file_name);
            if artifact.integrity.verify(&path).is_ok() {
                self.reporter.report(ProgressEvent::CacheHit {
                    name: &artifact.file_name,
                });
                return Ok(path);
            }
            fs::remove_file(&path).map_err(InstallerError::Io)?;
        }

        let temporary = tempfile::Builder::new()
            .prefix(".jolter-download-")
            .tempfile_in(&directory)
            .map_err(InstallerError::Io)?;
        self.http.get()?.download(
            &artifact.url,
            temporary.path(),
            &artifact.file_name,
            self.reporter.as_ref(),
        )?;
        self.report_stage(ProgressAction::Verify, &artifact.file_name);
        artifact.integrity.verify(temporary.path())?;
        temporary
            .persist(&path)
            .map_err(|error| InstallerError::Io(error.error))?;
        Ok(path)
    }

    pub(crate) fn metadata_text(&self, url: &str) -> Result<String, InstallerError> {
        self.metadata_text_inner(url, false)
    }

    pub(crate) fn npm_metadata_text(&self, url: &str) -> Result<String, InstallerError> {
        self.metadata_text_inner(url, true)
    }

    fn metadata_text_inner(&self, url: &str, npm_metadata: bool) -> Result<String, InstallerError> {
        ensure_https(url)?;
        let directory = self.storage.cache_dir().join("metadata");
        fs::create_dir_all(&directory).map_err(InstallerError::Io)?;
        let cache_source = if npm_metadata {
            format!("npm:{url}")
        } else {
            url.to_owned()
        };
        let cache_key = format!("{:x}", Sha256::digest(cache_source.as_bytes()));
        let metadata_lock = self.metadata_lock(&cache_key)?;
        FileExt::lock(&metadata_lock).map_err(InstallerError::Io)?;
        let cache_path = directory.join(format!("{cache_key}.txt"));
        let cached = fs::read_to_string(&cache_path).ok();
        let fresh = fs::metadata(&cache_path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age <= MAX_METADATA_CACHE_AGE_SECS);
        if fresh {
            return cached.ok_or_else(|| InstallerError::MetadataCacheRead {
                path: cache_path.clone(),
            });
        }
        if offline_mode() {
            return cached.ok_or_else(|| InstallerError::OfflineCacheMiss {
                url: url.to_owned(),
            });
        }

        let response = if npm_metadata {
            self.http.get()?.get_npm_metadata(url)
        } else {
            self.http.get()?.get_text(url)
        };
        match response {
            Ok(contents) => {
                write_cache_file(&cache_path, contents.as_bytes())?;
                Ok(contents)
            }
            Err(error) => cached.ok_or(error),
        }
    }

    fn report_stage(&self, action: ProgressAction, target: &str) {
        self.reporter
            .report(ProgressEvent::Stage { action, target });
    }
}

#[derive(Debug, Clone, Copy)]
enum InstallationType {
    Runtime,
    Tool,
}

fn remove_installation(
    storage: &Storage,
    path: PathBuf,
    expected_parent: &Path,
    installation_type: InstallationType,
) -> Result<RemovalOutcome, InstallerError> {
    if path.parent() != Some(expected_parent) {
        return Err(InstallerError::UnsafeRemoval { path });
    }
    if !path.exists() {
        return Err(match installation_type {
            InstallationType::Runtime => InstallerError::RuntimeNotInstalled { path },
            InstallationType::Tool => InstallerError::ToolNotInstalled { path },
        });
    }
    let CacheStats { bytes, .. } = storage.path_stats(&path)?;
    fs::remove_dir_all(&path).map_err(|source| InstallerError::RemoveInstallation {
        path: path.clone(),
        source,
    })?;
    Ok(RemovalOutcome {
        path,
        reclaimed_bytes: bytes,
    })
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), InstallerError> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)
        .map_err(InstallerError::Io)?
        .permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    fs::set_permissions(path, permissions).map_err(InstallerError::Io)
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
fn make_executable(_path: &Path) -> Result<(), InstallerError> {
    Ok(())
}

fn plugin_tool_executable(root: &Path, command: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let exe = root.join(format!("{command}.exe"));
        if exe.is_file() {
            return exe;
        }
    }
    root.join(command)
}

fn plugin_tool_commands_exist(root: &Path, commands: &[String]) -> bool {
    !commands.is_empty()
        && commands
            .iter()
            .all(|command| plugin_tool_executable(root, command).is_file())
}
