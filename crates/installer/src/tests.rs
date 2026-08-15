use super::*;
use base64::Engine;
use flate2::{Compression, write::GzEncoder};
use jolter_runtime::{RuntimeKind, ToolHash, ToolHashAlgorithm, ToolKind};
use jolter_storage::Storage;
use semver::Version;
use sha2::{Digest, Sha224, Sha256, Sha512};
use std::{
    collections::HashMap,
    fs,
    io::{self, Cursor, Write},
    path::Path,
    sync::{Arc, Mutex},
    thread,
};
use zip::{ZipWriter, write::SimpleFileOptions};

use archive::{strip_leading_components, validate_node_engine};
use http::{
    checksum_for, ensure_https, parse_checksum_value, parse_github_digest, retry_delay,
    retryable_status, verify_sha512, verify_tool_hash,
};
use installer::{write_manifest, write_plugin_tool_manifest, write_tool_manifest};
use providers::node::node_target;

struct FakeHttpClient {
    text: HashMap<String, String>,
    downloads: HashMap<String, Vec<u8>>,
    text_count: Mutex<usize>,
    download_count: Mutex<usize>,
}

#[derive(Default)]
struct RecordingReporter {
    events: Mutex<Vec<String>>,
}

impl ProgressReporter for RecordingReporter {
    fn report(&self, event: ProgressEvent<'_>) {
        let value = match event {
            ProgressEvent::Stage { action, target } => {
                format!("stage:{}:{target}", action.label())
            }
            ProgressEvent::DownloadStarted { name, total } => {
                format!("start:{name}:{total:?}")
            }
            ProgressEvent::DownloadAdvanced {
                name,
                downloaded,
                total,
            } => format!("advance:{name}:{downloaded}:{total:?}"),
            ProgressEvent::DownloadFinished {
                name,
                downloaded,
                total,
            } => format!("finish:{name}:{downloaded}:{total:?}"),
            ProgressEvent::CacheHit { name } => format!("cache:{name}"),
        };
        self.events.lock().unwrap().push(value);
    }
}

impl HttpClient for FakeHttpClient {
    fn get_text(&self, url: &str) -> Result<String, InstallerError> {
        *self.text_count.lock().unwrap() += 1;
        self.text.get(url).cloned().ok_or_else(|| {
            InstallerError::Io(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no fake response for {url}"),
            ))
        })
    }

    fn download(
        &self,
        url: &str,
        destination: &Path,
        name: &str,
        reporter: &dyn ProgressReporter,
    ) -> Result<(), InstallerError> {
        let bytes = self.downloads.get(url).ok_or_else(|| {
            InstallerError::Io(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no fake download for {url}"),
            ))
        })?;
        let total = Some(bytes.len() as u64);
        reporter.report(ProgressEvent::DownloadStarted { name, total });
        fs::write(destination, bytes).map_err(InstallerError::Io)?;
        *self.download_count.lock().unwrap() += 1;
        reporter.report(ProgressEvent::DownloadAdvanced {
            name,
            downloaded: bytes.len() as u64,
            total,
        });
        reporter.report(ProgressEvent::DownloadFinished {
            name,
            downloaded: bytes.len() as u64,
            total,
        });
        Ok(())
    }
}

#[test]
fn reports_download_bytes_verification_and_cache_reuse() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    storage.ensure_layout().unwrap();
    let bytes = b"verified archive".to_vec();
    let checksum = format!("{:x}", Sha256::digest(&bytes));
    let url = "https://example.test/archive.zip".to_owned();
    let client = Arc::new(FakeHttpClient {
        text: HashMap::new(),
        downloads: HashMap::from([(url.clone(), bytes.clone())]),
        text_count: Mutex::new(0),
        download_count: Mutex::new(0),
    });
    let reporter = Arc::new(RecordingReporter::default());
    let installer = Installer::with_client_and_reporter(
        storage,
        Platform::current().unwrap(),
        client,
        reporter.clone(),
    );
    let artifact = Artifact {
        url,
        integrity: ArtifactIntegrity::Sha256(checksum),
        file_name: "archive.zip".to_owned(),
        format: ArchiveFormat::Zip,
        strip_components: 0,
    };

    let first = installer.obtain_archive(&artifact).unwrap();
    let second = installer.obtain_archive(&artifact).unwrap();

    assert_eq!(first, second);
    let events = reporter.events.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|event| event == "start:archive.zip:Some(16)")
    );
    assert!(
        events
            .iter()
            .any(|event| event == "advance:archive.zip:16:Some(16)")
    );
    assert!(
        events
            .iter()
            .any(|event| event == "finish:archive.zip:16:Some(16)")
    );
    assert!(
        events
            .iter()
            .any(|event| event == "stage:verify:archive.zip")
    );
    assert!(events.iter().any(|event| event == "cache:archive.zip"));
}

#[test]
fn rejects_insecure_artifact() {
    let artifact = Artifact {
        url: "http://example.test/node.zip".to_owned(),
        integrity: ArtifactIntegrity::Sha256("a".repeat(64)),
        file_name: "node.zip".to_owned(),
        format: ArchiveFormat::Zip,
        strip_components: 0,
    };
    assert!(matches!(
        artifact.validate(),
        Err(InstallerError::InsecureUrl(_))
    ));
}

#[test]
fn selects_checksum_by_exact_file_name() {
    let sums = concat!(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  node-a.zip\n",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  node-b.zip\n"
    );
    assert_eq!(checksum_for(sums, "node-b.zip").unwrap(), "b".repeat(64));
}

#[test]
fn rejects_archive_parent_traversal() {
    assert!(matches!(
        strip_leading_components(Path::new("../outside"), 0),
        Err(InstallerError::UnsafeArchivePath(_))
    ));
}

#[test]
fn maps_supported_platform_assets() {
    let windows = Platform {
        os: OperatingSystem::Windows,
        arch: Architecture::X64,
        bun_cpu: BunCpu::Standard,
    };
    assert_eq!(
        GithubRuntime::Bun.asset_name(windows).unwrap(),
        "bun-windows-x64.zip"
    );
    assert_eq!(
        GithubRuntime::Deno.asset_name(windows).unwrap(),
        "deno-x86_64-pc-windows-msvc.zip"
    );
    let baseline = Platform {
        bun_cpu: BunCpu::Baseline,
        ..windows
    };
    assert_eq!(
        GithubRuntime::Bun.asset_name(baseline).unwrap(),
        "bun-windows-x64-baseline.zip"
    );
    let linux_arm = Platform {
        os: OperatingSystem::Linux,
        arch: Architecture::Arm64,
        bun_cpu: BunCpu::Standard,
    };
    assert_eq!(
        GithubRuntime::Bun.asset_name(linux_arm).unwrap(),
        "bun-linux-aarch64.zip"
    );
    assert_eq!(
        GithubRuntime::Deno.asset_name(linux_arm).unwrap(),
        "deno-aarch64-unknown-linux-gnu.zip"
    );
    let mac_x64 = Platform {
        os: OperatingSystem::MacOs,
        arch: Architecture::X64,
        bun_cpu: BunCpu::Standard,
    };
    assert_eq!(
        GithubRuntime::Deno.asset_name(mac_x64).unwrap(),
        "deno-x86_64-apple-darwin.zip"
    );
    let unsupported = Platform {
        bun_cpu: BunCpu::Unsupported,
        ..windows
    };
    assert!(matches!(
        GithubRuntime::Bun.asset_name(unsupported),
        Err(InstallerError::UnsupportedBunCpu)
    ));

    for (platform, expected) in [
        (
            Platform {
                os: OperatingSystem::Windows,
                arch: Architecture::Arm64,
                bun_cpu: BunCpu::Standard,
            },
            "win-arm64-zip",
        ),
        (
            Platform {
                os: OperatingSystem::Linux,
                arch: Architecture::X64,
                bun_cpu: BunCpu::Standard,
            },
            "linux-x64",
        ),
        (linux_arm, "linux-arm64"),
        (mac_x64, "osx-x64-tar"),
        (
            Platform {
                os: OperatingSystem::MacOs,
                arch: Architecture::Arm64,
                bun_cpu: BunCpu::Standard,
            },
            "osx-arm64-tar",
        ),
    ] {
        assert_eq!(node_target(platform).index_name, expected);
    }
}

#[test]
fn resolves_node_lts_from_the_official_index_shape() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    let platform = Platform::current().unwrap();
    let target = node_target(platform);
    let tagged_version = "v24.3.0";
    let file_name = format!(
        "node-{tagged_version}-{}.{}",
        target.archive_name,
        target.format.cache_extension()
    );
    let sums_url = format!("https://nodejs.org/dist/{tagged_version}/SHASUMS256.txt");
    let checksum = "b".repeat(64);
    let metadata = serde_json::json!([
        {
            "version": "v25.0.0",
            "lts": false,
            "files": [target.index_name]
        },
        {
            "version": tagged_version,
            "lts": "Krypton",
            "files": [target.index_name]
        }
    ])
    .to_string();
    let client = Arc::new(FakeHttpClient {
        text: HashMap::from([
            (NODE_INDEX_URL.to_owned(), metadata),
            (
                sums_url,
                format!("{checksum}  {file_name}\n{}  other.zip", "a".repeat(64)),
            ),
        ]),
        downloads: HashMap::new(),
        text_count: Mutex::new(0),
        download_count: Mutex::new(0),
    });
    let installer = Installer::with_client(storage, platform, client);

    let release = installer.resolve(&"node@lts".parse().unwrap()).unwrap();

    assert_eq!(release.kind, RuntimeKind::Node);
    assert_eq!(release.version, Version::new(24, 3, 0));
    assert_eq!(release.artifact.file_name, file_name);
    assert_eq!(
        release.artifact.integrity,
        ArtifactIntegrity::Sha256(checksum)
    );
}

#[test]
fn resolves_bun_and_deno_fallback_checksums() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    let platform = Platform {
        os: OperatingSystem::Windows,
        arch: Architecture::X64,
        bun_cpu: BunCpu::Standard,
    };
    let bun_asset = GithubRuntime::Bun.asset_name(platform).unwrap();
    let deno_asset = GithubRuntime::Deno.asset_name(platform).unwrap();
    let bun_download = format!("https://example.test/{bun_asset}");
    let deno_download = format!("https://example.test/{deno_asset}");
    let bun_sums = "https://example.test/SHASUMS256.txt".to_owned();
    let deno_sum = format!("https://example.test/{deno_asset}.sha256sum");
    let bun_checksum = "b".repeat(64);
    let deno_checksum = "d".repeat(64);
    let bun_metadata_url = format!("{GITHUB_API}/repos/oven-sh/bun/releases?per_page=100&page=1");
    let deno_metadata_url =
        format!("{GITHUB_API}/repos/denoland/deno/releases?per_page=100&page=1");
    let bun_metadata = serde_json::json!([{
        "tag_name": "bun-v1.3.2",
        "draft": false,
        "prerelease": false,
        "assets": [
            {
                "name": bun_asset.clone(),
                "browser_download_url": bun_download,
                "digest": null
            },
            {
                "name": "SHASUMS256.txt",
                "browser_download_url": bun_sums,
                "digest": null
            }
        ]
    }])
    .to_string();
    let deno_metadata = serde_json::json!([{
        "tag_name": "v2.4.1",
        "draft": false,
        "prerelease": false,
        "assets": [
            {
                "name": deno_asset.clone(),
                "browser_download_url": deno_download,
                "digest": null
            },
            {
                "name": format!("{deno_asset}.sha256sum"),
                "browser_download_url": deno_sum,
                "digest": null
            }
        ]
    }])
    .to_string();
    let client = Arc::new(FakeHttpClient {
        text: HashMap::from([
            (bun_metadata_url, bun_metadata),
            (
                "https://example.test/SHASUMS256.txt".to_owned(),
                format!("{bun_checksum} *{bun_asset}"),
            ),
            (deno_metadata_url, deno_metadata),
            (
                format!("https://example.test/{deno_asset}.sha256sum"),
                format!("{deno_checksum}  {deno_asset}"),
            ),
        ]),
        downloads: HashMap::new(),
        text_count: Mutex::new(0),
        download_count: Mutex::new(0),
    });
    let installer = Installer::with_client(storage, platform, client);

    let bun = installer.resolve(&"bun@1".parse().unwrap()).unwrap();
    let deno = installer.resolve(&"deno@2".parse().unwrap()).unwrap();

    assert_eq!(bun.version, Version::new(1, 3, 2));
    assert_eq!(
        bun.artifact.integrity,
        ArtifactIntegrity::Sha256(bun_checksum)
    );
    assert_eq!(deno.version, Version::new(2, 4, 1));
    assert_eq!(
        deno.artifact.integrity,
        ArtifactIntegrity::Sha256(deno_checksum)
    );
}

#[test]
fn installs_and_repairs_a_verified_runtime_archive() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    let platform = Platform::current().unwrap();
    let asset_name = GithubRuntime::Deno.asset_name(platform).unwrap();
    let executable_name = if cfg!(windows) { "deno.exe" } else { "deno" };
    let archive = zip_with_file(executable_name, b"fake deno");
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let download_url = format!("https://example.test/{asset_name}");
    let metadata_url = format!("{GITHUB_API}/repos/denoland/deno/releases?per_page=100&page=1");
    let tag_url = format!("{GITHUB_API}/repos/denoland/deno/releases/tags/v2.8.3");
    let metadata = serde_json::json!([{
        "tag_name": "v2.8.3",
        "draft": false,
        "prerelease": false,
        "assets": [{
            "name": asset_name.clone(),
            "browser_download_url": download_url.clone(),
            "digest": format!("sha256:{checksum}")
        }]
    }])
    .to_string();
    let single_metadata = serde_json::json!({
        "tag_name": "v2.8.3",
        "draft": false,
        "prerelease": false,
        "assets": [{
            "name": asset_name.clone(),
            "browser_download_url": download_url.clone(),
            "digest": format!("sha256:{checksum}")
        }]
    })
    .to_string();
    let client = Arc::new(FakeHttpClient {
        text: HashMap::from([(metadata_url, metadata), (tag_url, single_metadata)]),
        downloads: HashMap::from([(download_url, archive)]),
        text_count: Mutex::new(0),
        download_count: Mutex::new(0),
    });
    let installer = Installer::with_client(storage.clone(), platform, client.clone());

    let outcome = installer.install(&"deno@2".parse().unwrap()).unwrap();
    assert!(outcome.downloaded);
    assert!(
        storage
            .runtime_executable(jolter_runtime::RuntimeKind::Deno, &Version::new(2, 8, 3))
            .is_file()
    );
    assert!(outcome.runtime.path.join(".jolter-install.json").is_file());

    fs::remove_file(
        storage.runtime_executable(jolter_runtime::RuntimeKind::Deno, &Version::new(2, 8, 3)),
    )
    .unwrap();
    let repaired = installer.repair(&"deno@2.8.3".parse().unwrap()).unwrap();
    assert!(repaired.downloaded);
    assert_eq!(*client.text_count.lock().unwrap(), 2);
    assert_eq!(*client.download_count.lock().unwrap(), 1);
}

#[test]
fn installs_a_verified_tool_archive() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    let archive = tar_gz_with_file("package/bin/pnpm.cjs", b"fake pnpm");
    let integrity = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(&archive))
    );
    let corepack_hash = format!("{:x}", Sha224::digest(&archive));
    let metadata_url = "https://registry.npmjs.org/pnpm".to_owned();
    let download_url = "https://registry.npmjs.org/pnpm/-/pnpm-10.2.0.tgz".to_owned();
    let metadata = serde_json::json!({
        "dist-tags": { "latest": "10.2.0" },
        "versions": {
            "10.1.0": {
                "version": "10.1.0",
                "dist": {
                    "tarball": "https://registry.npmjs.org/pnpm/-/pnpm-10.1.0.tgz",
                    "integrity": integrity
                }
            },
            "10.2.0": {
                "version": "10.2.0",
                "dist": {
                    "tarball": download_url.clone(),
                    "integrity": integrity
                }
            }
        }
    })
    .to_string();
    let client = Arc::new(FakeHttpClient {
        text: HashMap::from([(metadata_url, metadata)]),
        downloads: HashMap::from([(download_url, archive)]),
        text_count: Mutex::new(0),
        download_count: Mutex::new(0),
    });
    let installer = Installer::with_client(
        storage.clone(),
        Platform::current().unwrap(),
        client.clone(),
    );
    let request: ToolRequest = format!("pnpm@10.2.0+sha224.{corepack_hash}")
        .parse()
        .unwrap();

    let outcome = installer
        .install_tool(&request, &Version::new(20, 0, 0))
        .unwrap();

    assert!(outcome.downloaded);
    assert_eq!(outcome.tool.version, Version::new(10, 2, 0));
    assert!(
        storage
            .tool_entrypoint(ToolKind::Pnpm, &outcome.tool.version, "pnpm")
            .unwrap()
            .is_file()
    );
    assert!(outcome.tool.path.join(".jolter-tool.json").is_file());
    let manifest = fs::read_to_string(outcome.tool.path.join(".jolter-tool.json")).unwrap();
    assert!(manifest.contains(&format!("sha224.{corepack_hash}")));
    assert_eq!(*client.download_count.lock().unwrap(), 1);

    let reused = installer
        .install_tool(&"pnpm@latest".parse().unwrap(), &Version::new(20, 0, 0))
        .unwrap();
    assert!(!reused.downloaded);
}

#[test]
fn repairs_and_uninstalls_tools_and_cleans_cache() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    storage.ensure_layout().unwrap();
    let archive = tar_gz_with_file("package/bin/pnpm.cjs", b"fake pnpm");
    let integrity = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(&archive))
    );
    let metadata_url = "https://registry.npmjs.org/pnpm".to_owned();
    let download_url = "https://registry.npmjs.org/pnpm/-/pnpm-10.2.0.tgz".to_owned();
    let metadata = serde_json::json!({
        "dist-tags": { "latest": "10.2.0" },
        "versions": {
            "10.2.0": {
                "version": "10.2.0",
                "dist": {
                    "tarball": download_url.clone(),
                    "integrity": integrity
                }
            }
        }
    })
    .to_string();
    let client = Arc::new(FakeHttpClient {
        text: HashMap::from([(metadata_url, metadata)]),
        downloads: HashMap::from([(download_url, archive)]),
        text_count: Mutex::new(0),
        download_count: Mutex::new(0),
    });
    let installer = Installer::with_client(storage.clone(), Platform::current().unwrap(), client);
    let version = Version::new(10, 2, 0);
    fs::create_dir_all(storage.tool_version_dir(ToolKind::Pnpm, &version)).unwrap();

    assert!(matches!(
        installer.install_tool(&"pnpm@10.2.0".parse().unwrap(), &Version::new(20, 0, 0)),
        Err(InstallerError::CorruptToolInstallation { .. })
    ));
    let repaired = installer
        .repair_tool(&"pnpm@10.2.0".parse().unwrap(), &Version::new(20, 0, 0))
        .unwrap();
    assert!(repaired.downloaded);
    let removed = installer.uninstall_tool(ToolKind::Pnpm, &version).unwrap();
    assert!(removed.reclaimed_bytes > 0);
    assert!(matches!(
        installer.uninstall_tool(ToolKind::Pnpm, &version),
        Err(InstallerError::ToolNotInstalled { .. })
    ));

    let metadata_cache = storage.cache_dir().join("metadata").join("orphan.txt");
    fs::create_dir_all(metadata_cache.parent().unwrap()).unwrap();
    fs::write(&metadata_cache, b"metadata").unwrap();
    let cleaned = installer.clean_cache().unwrap();
    assert!(cleaned.removed_files >= 2);
    assert!(!metadata_cache.exists());
}

#[test]
fn concurrent_runtime_installation_downloads_once() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    let platform = Platform::current().unwrap();
    let asset_name = GithubRuntime::Deno.asset_name(platform).unwrap();
    let executable_name = if cfg!(windows) { "deno.exe" } else { "deno" };
    let archive = zip_with_file(executable_name, b"fake deno");
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let download_url = format!("https://example.test/{asset_name}");
    let metadata_url = format!("{GITHUB_API}/repos/denoland/deno/releases?per_page=100&page=1");
    let metadata = serde_json::json!([{
        "tag_name": "v2.8.3",
        "draft": false,
        "prerelease": false,
        "assets": [{
            "name": asset_name,
            "browser_download_url": download_url.clone(),
            "digest": format!("sha256:{checksum}")
        }]
    }])
    .to_string();
    let client = Arc::new(FakeHttpClient {
        text: HashMap::from([(metadata_url, metadata)]),
        downloads: HashMap::from([(download_url, archive)]),
        text_count: Mutex::new(0),
        download_count: Mutex::new(0),
    });
    let first = Installer::with_client(storage.clone(), platform, client.clone());
    let second = Installer::with_client(storage, platform, client.clone());

    let first = thread::spawn(move || first.install(&"deno@2".parse().unwrap()).unwrap());
    let second = thread::spawn(move || second.install(&"deno@2".parse().unwrap()).unwrap());
    let outcomes = [first.join().unwrap(), second.join().unwrap()];

    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.downloaded).count(),
        1
    );
    assert_eq!(*client.download_count.lock().unwrap(), 1);
}

#[test]
fn rejects_a_mismatched_corepack_hash_before_publication() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    let archive = tar_gz_with_file("package/bin/pnpm.cjs", b"fake pnpm");
    let integrity = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(&archive))
    );
    let metadata_url = "https://registry.npmjs.org/pnpm".to_owned();
    let download_url = "https://registry.npmjs.org/pnpm/-/pnpm-10.2.0.tgz".to_owned();
    let metadata = serde_json::json!({
        "dist-tags": { "latest": "10.2.0" },
        "versions": {
            "10.2.0": {
                "version": "10.2.0",
                "dist": {
                    "tarball": download_url.clone(),
                    "integrity": integrity
                }
            }
        }
    })
    .to_string();
    let client = Arc::new(FakeHttpClient {
        text: HashMap::from([(metadata_url, metadata)]),
        downloads: HashMap::from([(download_url, archive)]),
        text_count: Mutex::new(0),
        download_count: Mutex::new(0),
    });
    let installer = Installer::with_client(storage.clone(), Platform::current().unwrap(), client);
    let request: ToolRequest = format!("pnpm@10.2.0+sha224.{}", "0".repeat(56))
        .parse()
        .unwrap();

    let error = installer
        .install_tool(&request, &Version::new(20, 0, 0))
        .unwrap_err();

    assert!(matches!(error, InstallerError::ToolHashMismatch { .. }));
    assert!(
        !storage
            .tool_version_dir(ToolKind::Pnpm, &Version::new(10, 2, 0))
            .exists()
    );
}

#[test]
fn rejects_a_tool_incompatible_with_selected_node() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    let metadata_url = "https://registry.npmjs.org/pnpm".to_owned();
    let metadata = serde_json::json!({
        "dist-tags": { "latest": "11.6.0" },
        "versions": {
            "11.6.0": {
                "version": "11.6.0",
                "dist": {
                    "tarball": "https://registry.npmjs.org/pnpm/-/pnpm-11.6.0.tgz",
                    "integrity": format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode([0_u8; 64]))
                },
                "engines": {
                    "node": ">=22.13"
                }
            }
        }
    })
    .to_string();
    let client = Arc::new(FakeHttpClient {
        text: HashMap::from([(metadata_url, metadata)]),
        downloads: HashMap::new(),
        text_count: Mutex::new(0),
        download_count: Mutex::new(0),
    });
    let installer = Installer::with_client(storage, Platform::current().unwrap(), client.clone());

    let error = installer
        .install_tool(&"pnpm@11.6.0".parse().unwrap(), &Version::new(20, 19, 0))
        .unwrap_err();

    assert!(matches!(
        error,
        InstallerError::IncompatibleNodeVersion { .. }
    ));
    assert_eq!(*client.download_count.lock().unwrap(), 0);
}

#[test]
fn evaluates_npm_style_node_engine_ranges() {
    validate_node_engine(
        ToolKind::Pnpm,
        &Version::new(10, 2, 0),
        Some("^18.18.0 || >=20.9.0"),
        &Version::new(20, 10, 0),
    )
    .unwrap();

    let error = validate_node_engine(
        ToolKind::Pnpm,
        &Version::new(10, 2, 0),
        Some("^18.18.0 || >=20.9.0"),
        &Version::new(19, 0, 0),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        InstallerError::IncompatibleNodeVersion { .. }
    ));
}

fn zip_with_file(path: &str, contents: &[u8]) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut cursor);
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
        writer.finish().unwrap();
    }
    cursor.into_inner()
}

fn tar_gz_with_file(path: &str, contents: &[u8]) -> Vec<u8> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = tar::Builder::new(encoder);
    let mut header = tar::Header::new_gnu();
    header.set_size(contents.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    archive.append_data(&mut header, path, contents).unwrap();
    archive.into_inner().unwrap().finish().unwrap()
}

#[test]
fn retries_only_transient_http_statuses_with_bounded_delays() {
    assert!(retryable_status(reqwest::StatusCode::TOO_MANY_REQUESTS));
    assert!(retryable_status(reqwest::StatusCode::BAD_GATEWAY));
    assert!(retryable_status(reqwest::StatusCode::SERVICE_UNAVAILABLE));
    assert!(retryable_status(reqwest::StatusCode::GATEWAY_TIMEOUT));
    assert!(retryable_status(reqwest::StatusCode::REQUEST_TIMEOUT));
    assert!(retryable_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR));
    assert!(!retryable_status(reqwest::StatusCode::OK));
    assert!(!retryable_status(reqwest::StatusCode::NOT_FOUND));

    let delay0 = retry_delay(0, None);
    assert_eq!(delay0, std::time::Duration::from_millis(250));
    let delay1 = retry_delay(1, None);
    assert_eq!(delay1, std::time::Duration::from_millis(500));
    let delay2 = retry_delay(2, None);
    assert_eq!(delay2, std::time::Duration::from_secs(1));

    let header = reqwest::header::HeaderValue::from_static("3");
    let delay_header = retry_delay(0, Some(&header));
    assert_eq!(delay_header, std::time::Duration::from_secs(3));

    let header_large = reqwest::header::HeaderValue::from_static("60");
    let delay_clamped = retry_delay(0, Some(&header_large));
    assert_eq!(delay_clamped, MAX_RETRY_AFTER);
}

#[test]
fn validates_integrity_and_checksum_error_paths() {
    assert!(matches!(
        ArtifactIntegrity::from_sri("sha256-deadbeef"),
        Err(InstallerError::UnsupportedIntegrity(_))
    ));
    assert!(matches!(
        ArtifactIntegrity::from_sri("sha512-not-base64"),
        Err(InstallerError::InvalidIntegrity(_))
    ));
    assert!(matches!(
        Artifact {
            url: "https://example.test/archive.zip".to_owned(),
            integrity: ArtifactIntegrity::Sha256("a".repeat(64)),
            file_name: "../archive.zip".to_owned(),
            format: ArchiveFormat::Zip,
            strip_components: 0,
        }
        .validate(),
        Err(InstallerError::InvalidArtifactName(_))
    ));
    assert!(matches!(
        checksum_for("", "missing.zip"),
        Err(InstallerError::ChecksumNotFound { .. })
    ));
    assert!(matches!(
        parse_checksum_value(""),
        Err(InstallerError::EmptyChecksum)
    ));

    let temp = tempfile::NamedTempFile::new().unwrap();
    fs::write(temp.path(), b"contents").unwrap();
    assert!(matches!(
        verify_sha256(temp.path(), &"0".repeat(64)),
        Err(InstallerError::ChecksumMismatch { .. })
    ));
    let wrong_sha512 = base64::engine::general_purpose::STANDARD.encode([0_u8; 64]);
    assert!(matches!(
        verify_sha512(temp.path(), &wrong_sha512),
        Err(InstallerError::ChecksumMismatch { .. })
    ));
    for algorithm in [
        ToolHashAlgorithm::Sha1,
        ToolHashAlgorithm::Sha256,
        ToolHashAlgorithm::Sha384,
        ToolHashAlgorithm::Sha512,
    ] {
        let expected = ToolHash {
            algorithm,
            value: "0".repeat(algorithm.hex_length()),
        };
        assert!(matches!(
            verify_tool_hash(temp.path(), &expected),
            Err(InstallerError::ToolHashMismatch { .. })
        ));
    }
}

#[test]
fn tests_manifest_writing_and_error_display() {
    let temp = tempfile::tempdir().unwrap();
    let release = Release {
        kind: jolter_runtime::RuntimeKind::Node,
        version: Version::new(20, 0, 0),
        artifact: Artifact {
            url: "https://nodejs.org/dist/v20.0.0/node-v20.0.0-win-x64.zip".to_owned(),
            integrity: ArtifactIntegrity::Sha256("a".repeat(64)),
            file_name: "node.zip".to_owned(),
            format: ArchiveFormat::Zip,
            strip_components: 1,
        },
    };

    write_manifest(temp.path(), &release).unwrap();
    assert!(temp.path().join(".jolter-install.json").is_file());

    let tool_release = ToolRelease {
        kind: ToolKind::Pnpm,
        version: Version::new(10, 0, 0),
        artifact: Artifact {
            url: "https://registry.npmjs.org/pnpm/-/pnpm-10.0.0.tgz".to_owned(),
            integrity: ArtifactIntegrity::Sha256("b".repeat(64)),
            file_name: "pnpm.tgz".to_owned(),
            format: ArchiveFormat::TarGz,
            strip_components: 1,
        },
        node_engine: Some(">=18".to_owned()),
        expected_hash: None,
    };

    write_tool_manifest(temp.path(), &tool_release).unwrap();
    assert!(temp.path().join(".jolter-tool.json").is_file());

    let plugin_tool_archive = PluginToolArchive {
        provider: "my-provider".to_owned(),
        tool: "my-tool".to_owned(),
        version: Version::new(1, 0, 0),
        artifact: Artifact {
            url: "https://example.test/tool.zip".to_owned(),
            integrity: ArtifactIntegrity::Sha256("c".repeat(64)),
            file_name: "tool.zip".to_owned(),
            format: ArchiveFormat::Zip,
            strip_components: 0,
        },
        commands: vec!["my-tool".to_owned()],
    };

    write_plugin_tool_manifest(temp.path(), &plugin_tool_archive).unwrap();
    assert!(temp.path().join(".jolter-plugin-tool.json").is_file());

    let err = InstallerError::InsecureUrl("http://insecure.test".to_owned());
    assert_eq!(
        err.to_string(),
        "refusing non-HTTPS URL `http://insecure.test`"
    );

    let err = InstallerError::ArchiveSizeLimit;
    assert_eq!(err.to_string(), "archive exceeded the extracted size limit");
}

#[test]
fn tests_unsafe_archive_paths_and_urls() {
    assert!(matches!(
        strip_leading_components(Path::new("../outside.txt"), 0),
        Err(InstallerError::UnsafeArchivePath(_))
    ));
    assert!(matches!(
        strip_leading_components(Path::new("/etc/passwd"), 0),
        Err(InstallerError::UnsafeArchivePath(_))
    ));
    assert!(strip_leading_components(Path::new("valid/sub/path.txt"), 0).is_ok());

    assert!(ensure_https("https://secure.test").is_ok());
    assert!(matches!(
        ensure_https("http://insecure.test"),
        Err(InstallerError::InsecureUrl(_))
    ));
}

#[test]
fn tests_release_channel_and_self_upgrade() {
    assert_eq!(ReleaseChannel::default(), ReleaseChannel::Stable);
    assert_eq!(ReleaseChannel::Stable.to_string(), "stable");
    assert_eq!(ReleaseChannel::Nightly.to_string(), "nightly");

    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    storage.ensure_layout().unwrap();

    let mut text = HashMap::new();
    text.insert(
        "https://api.github.com/repos/jolterjs/jolter/releases/latest".to_owned(),
        r#"{"tag_name":"v0.3.0"}"#.to_owned(),
    );
    text.insert(
        "https://api.github.com/repos/jolterjs/jolter/releases".to_owned(),
        r#"[{"tag_name":"v0.4.0-nightly.1","draft":false,"prerelease":true}]"#.to_owned(),
    );
    text.insert(
        "https://api.github.com/repos/jolterjs/jolter/releases/tags/nightly".to_owned(),
        r#"{"tag_name":"v0.4.0-nightly.1"}"#.to_owned(),
    );

    let archive_bytes = b"fake binary payload".to_vec();
    let sha256_hex = format!("{:x}", Sha256::digest(&archive_bytes));

    let platform = Platform::current().unwrap();
    let (os_name, arch_name) = match (platform.os, platform.arch) {
        (OperatingSystem::Windows, Architecture::X64) => ("pc-windows-msvc", "x86_64"),
        (OperatingSystem::Windows, Architecture::Arm64) => ("pc-windows-msvc", "aarch64"),
        (OperatingSystem::Linux, Architecture::X64) => ("unknown-linux-gnu", "x86_64"),
        (OperatingSystem::Linux, Architecture::Arm64) => ("unknown-linux-gnu", "aarch64"),
        (OperatingSystem::MacOs, Architecture::X64) => ("apple-darwin", "x86_64"),
        (OperatingSystem::MacOs, Architecture::Arm64) => ("apple-darwin", "aarch64"),
    };
    let target_triple = format!("{arch_name}-{os_name}");
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };

    let stable_archive_name = format!("jolter-v0.3.0-{target_triple}.{ext}");
    let stable_download_url = format!(
        "https://github.com/jolterjs/jolter/releases/download/v0.3.0/{stable_archive_name}"
    );
    let stable_checksum_url = format!("{stable_download_url}.sha256");

    text.insert(stable_checksum_url, sha256_hex.clone());

    let nightly_archive_name = format!("jolter-v0.4.0-nightly.1-{target_triple}.{ext}");
    let nightly_download_url = format!(
        "https://github.com/jolterjs/jolter/releases/download/v0.4.0-nightly.1/{nightly_archive_name}"
    );
    let nightly_checksum_url = format!("{nightly_download_url}.sha256");

    text.insert(nightly_checksum_url, sha256_hex);

    let client = Arc::new(FakeHttpClient {
        text,
        downloads: HashMap::new(),
        text_count: Mutex::new(0),
        download_count: Mutex::new(0),
    });

    let reporter = Arc::new(RecordingReporter::default());
    let installer = Installer::with_client_and_reporter(storage, platform, client, reporter);

    let (ver_stable, artifact_stable) = installer
        .resolve_self_release(ReleaseChannel::Stable)
        .unwrap();
    assert_eq!(ver_stable, Version::new(0, 3, 0));
    assert_eq!(artifact_stable.url, stable_download_url);

    let (ver_nightly, artifact_nightly) = installer
        .resolve_self_release(ReleaseChannel::Nightly)
        .unwrap();
    assert_eq!(ver_nightly, Version::parse("0.4.0-nightly.1").unwrap());
    assert_eq!(artifact_nightly.url, nightly_download_url);
}

#[test]
fn tests_installer_constructors_and_lazy_client() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());

    let installer_default = Installer::new(storage.clone()).unwrap();
    assert_eq!(installer_default.storage.root(), storage.root());

    let lazy = installer::HttpClientSource::Lazy(Mutex::new(None));
    let client = lazy.get().unwrap();
    assert!(client.get_text("http://insecure.test").is_err());
}

#[test]
fn tests_installer_resolve_runtimes_and_http_helpers() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::new(temp.path());
    storage.ensure_layout().unwrap();

    let platform = Platform::current().unwrap();
    let bun_file = GithubRuntime::Bun.asset_name(platform).unwrap();
    let deno_file = GithubRuntime::Deno.asset_name(platform).unwrap();

    let bun_asset = format!(
        "{{\"name\":\"{bun_file}\",\"browser_download_url\":\"https://example.test/bun.zip\",\"digest\":null}}"
    );
    let bun_shasums_asset = "{\"name\":\"SHASUMS256.txt\",\"browser_download_url\":\"https://example.test/bun_shasums.txt\",\"digest\":null}";

    let deno_asset = format!(
        "{{\"name\":\"{deno_file}\",\"browser_download_url\":\"https://example.test/deno.zip\",\"digest\":null}}"
    );
    let deno_sha_asset = format!(
        "{{\"name\":\"{deno_file}.sha256sum\",\"browser_download_url\":\"https://example.test/deno_sha.txt\",\"digest\":null}}"
    );

    let dummy_hash = "a".repeat(64);

    let mut text = HashMap::new();
    text.insert(
        "https://api.github.com/repos/oven-sh/bun/releases/tags/bun-v1.1.0".to_owned(),
        format!("{{\"tag_name\":\"bun-v1.1.0\",\"draft\":false,\"prerelease\":false,\"assets\":[{bun_asset},{bun_shasums_asset}]}}"),
    );
    text.insert(
        "https://api.github.com/repos/oven-sh/bun/releases?per_page=100&page=1".to_owned(),
        format!("[{{\"tag_name\":\"bun-v1.1.0\",\"draft\":false,\"prerelease\":false,\"assets\":[{bun_asset},{bun_shasums_asset}]}}]"),
    );
    text.insert(
        "https://api.github.com/repos/denoland/deno/releases/tags/v1.40.0".to_owned(),
        format!("{{\"tag_name\":\"v1.40.0\",\"draft\":false,\"prerelease\":false,\"assets\":[{deno_asset},{deno_sha_asset}]}}"),
    );
    text.insert(
        "https://api.github.com/repos/denoland/deno/releases?per_page=100&page=1".to_owned(),
        format!("[{{\"tag_name\":\"v1.40.0\",\"draft\":false,\"prerelease\":false,\"assets\":[{deno_asset},{deno_sha_asset}]}}]"),
    );

    text.insert(
        "https://example.test/bun_shasums.txt".to_owned(),
        format!("{dummy_hash}  {bun_file}"),
    );
    text.insert(
        "https://example.test/deno_sha.txt".to_owned(),
        format!("{dummy_hash}  {deno_file}"),
    );
    let valid_sha512_sri = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(&[0_u8; 64])
    );
    text.insert(
        "https://registry.npmjs.org/pnpm".to_owned(),
        format!(
            r#"{{"dist-tags":{{"latest":"10.2.0"}},"versions":{{"10.2.0":{{"version":"10.2.0","dist":{{"tarball":"https://example.test/pnpm.tgz","integrity":"{valid_sha512_sri}"}}}}}}}}"#
        ),
    );

    let client = Arc::new(FakeHttpClient {
        text,
        downloads: HashMap::new(),
        text_count: Mutex::new(0),
        download_count: Mutex::new(0),
    });

    let platform = Platform::current().unwrap();
    let installer = Installer::with_client(storage.clone(), platform, client);

    let bun_req = jolter_runtime::RuntimeRequest::new(RuntimeKind::Bun, "1.1.0").unwrap();
    let res_bun = installer.resolve(&bun_req).unwrap();
    assert_eq!(res_bun.version, Version::new(1, 1, 0));

    let deno_req = jolter_runtime::RuntimeRequest::new(RuntimeKind::Deno, "1.40.0").unwrap();
    let res_deno = installer.resolve(&deno_req).unwrap();
    assert_eq!(res_deno.version, Version::new(1, 40, 0));

    let ver_99 = Version::new(99, 9, 9);
    assert!(matches!(
        installer.uninstall_runtime(RuntimeKind::Node, &ver_99),
        Err(InstallerError::RuntimeNotInstalled { .. })
    ));

    assert!(matches!(
        installer.uninstall_tool(ToolKind::Pnpm, &ver_99),
        Err(InstallerError::ToolNotInstalled { .. })
    ));

    let v10 = Version::new(10, 0, 0);
    let v24 = Version::new(24, 0, 0);
    assert!(matches!(
        validate_node_engine(ToolKind::Pnpm, &v10, Some("invalid range syntax!"), &v24),
        Err(InstallerError::InvalidNodeEngineRange { .. })
    ));

    assert!(matches!(
        validate_node_engine(ToolKind::Pnpm, &v10, Some(">=30.0.0"), &v24),
        Err(InstallerError::IncompatibleNodeVersion { .. })
    ));

    let temp_file = temp.path().join("hash_test.bin");
    fs::write(&temp_file, b"test_payload_data").unwrap();

    let sha1_val = format!("{:x}", sha1::Sha1::digest(b"test_payload_data"));
    let tool_hash_sha1 = ToolHash {
        algorithm: ToolHashAlgorithm::Sha1,
        value: sha1_val.clone(),
    };
    assert!(verify_tool_hash(&temp_file, &tool_hash_sha1).is_ok());

    let sha224_val = format!("{:x}", sha2::Sha224::digest(b"test_payload_data"));
    let tool_hash_sha224 = ToolHash {
        algorithm: ToolHashAlgorithm::Sha224,
        value: sha224_val,
    };
    assert!(verify_tool_hash(&temp_file, &tool_hash_sha224).is_ok());

    let sha384_val = format!("{:x}", sha2::Sha384::digest(b"test_payload_data"));
    let tool_hash_sha384 = ToolHash {
        algorithm: ToolHashAlgorithm::Sha384,
        value: sha384_val,
    };
    assert!(verify_tool_hash(&temp_file, &tool_hash_sha384).is_ok());

    let sha512_bytes = sha2::Sha512::digest(b"test_payload_data");
    let sha512_b64 = base64::engine::general_purpose::STANDARD.encode(sha512_bytes);
    assert!(verify_sha512(&temp_file, &sha512_b64).is_ok());

    assert!(verify_sha512(&temp_file, "invalid_base64!").is_err());

    let sha256_val = format!("{:x}", sha2::Sha256::digest(b"test_payload_data"));
    assert_eq!(
        parse_github_digest(&format!("sha256:{sha256_val}")),
        Some(sha256_val)
    );
    assert!(parse_github_digest("sha256:invalid").is_none());

    assert!(matches!(
        parse_checksum_value(""),
        Err(InstallerError::EmptyChecksum)
    ));

    let platforms = [
        Platform {
            os: OperatingSystem::Windows,
            arch: Architecture::X64,
            bun_cpu: types::BunCpu::Standard,
        },
        Platform {
            os: OperatingSystem::Windows,
            arch: Architecture::Arm64,
            bun_cpu: types::BunCpu::Standard,
        },
        Platform {
            os: OperatingSystem::Linux,
            arch: Architecture::X64,
            bun_cpu: types::BunCpu::Standard,
        },
        Platform {
            os: OperatingSystem::Linux,
            arch: Architecture::Arm64,
            bun_cpu: types::BunCpu::Standard,
        },
        Platform {
            os: OperatingSystem::MacOs,
            arch: Architecture::X64,
            bun_cpu: types::BunCpu::Standard,
        },
        Platform {
            os: OperatingSystem::MacOs,
            arch: Architecture::Arm64,
            bun_cpu: types::BunCpu::Standard,
        },
    ];

    for p in platforms {
        let target = node_target(p);
        assert!(!target.index_name.is_empty());
        assert!(!target.archive_name.is_empty());
    }

    let pnpm_range_req: ToolRequest = "pnpm@10.x".parse().unwrap();
    let res_pnpm_range = installer.resolve_tool(&pnpm_range_req).unwrap();
    assert_eq!(res_pnpm_range.version, Version::new(10, 2, 0));

    assert!(matches!(
        validate_node_engine(
            ToolKind::Pnpm,
            &Version::new(1, 0, 0),
            Some("invalid_range_syntax!"),
            &Version::new(24, 0, 0),
        ),
        Err(InstallerError::InvalidNodeEngineRange { .. })
    ));

    let zip_file = temp.path().join("archive.zip");
    {
        let file = fs::File::create(&zip_file).unwrap();
        let mut zip_writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip_writer.add_directory("root/", options).unwrap();
        zip_writer.start_file("root/content.txt", options).unwrap();
        zip_writer.write_all(b"zip_content").unwrap();
        zip_writer.finish().unwrap();
    }
    let extract_dir = temp.path().join("zip_out");
    assert!(archive::extract_archive(&zip_file, &extract_dir, ArchiveFormat::Zip, 1).is_ok());
    assert_eq!(
        fs::read_to_string(extract_dir.join("content.txt")).unwrap(),
        "zip_content"
    );

    assert!(matches!(
        installer.uninstall_runtime(RuntimeKind::Node, &Version::new(99, 0, 0)),
        Err(InstallerError::RuntimeNotInstalled { .. })
    ));

    assert!(matches!(
        installer.uninstall_tool(jolter_runtime::ToolKind::Pnpm, &Version::new(99, 0, 0)),
        Err(InstallerError::ToolNotInstalled { .. })
    ));

    assert!(matches!(
        installer.uninstall_plugin_tool("provider", "eslint-cli", &Version::new(99, 0, 0)),
        Err(InstallerError::ToolNotInstalled { .. })
    ));

    assert!(matches!(
        http::ensure_https("http://example.test"),
        Err(InstallerError::InsecureUrl(_))
    ));
    assert!(matches!(
        http::ensure_https("invalid url syntax"),
        Err(InstallerError::InvalidUrl(_))
    ));
    assert!(matches!(
        http::validate_checksum("short_hex"),
        Err(InstallerError::InvalidChecksum(_))
    ));

    assert!(matches!(
        archive::strip_leading_components(Path::new("a/../b"), 0),
        Err(InstallerError::UnsafeArchivePath(_))
    ));
    assert!(
        archive::strip_leading_components(Path::new("a/b"), 5)
            .unwrap()
            .is_none()
    );

    #[cfg(unix)]
    assert!(matches!(
        archive::validate_symlink_target(Path::new("a/b"), Path::new("../../outside")),
        Err(InstallerError::UnsafeArchivePath(_))
    ));

    let tool_invalid_pj = jolter_storage::InstalledTool {
        kind: jolter_runtime::ToolKind::Pnpm,
        version: Version::new(10, 0, 0),
        path: temp.path().join("tool_invalid_pj"),
    };
    fs::create_dir_all(&tool_invalid_pj.path).unwrap();
    fs::write(tool_invalid_pj.path.join("package.json"), b"invalid_json!").unwrap();
    assert!(matches!(
        installer.validate_installed_tool(&tool_invalid_pj, &Version::new(24, 0, 0)),
        Err(InstallerError::InstalledPackageMetadataParse { .. })
    ));

    let missing_runtime = jolter_storage::InstalledRuntime {
        kind: jolter_runtime::RuntimeKind::Node,
        version: Version::new(99, 0, 0),
        path: temp.path().join("runtimes/node/99.0.0"),
    };
    assert!(matches!(
        installer.uninstall_runtime(missing_runtime.kind, &missing_runtime.version),
        Err(InstallerError::RuntimeNotInstalled { .. })
    ));

    assert!(matches!(
        http::checksum_for(
            "1234567890123456789012345678901234567890123456789012345678901234 other.tar.gz",
            "target.tar.gz"
        ),
        Err(InstallerError::ChecksumNotFound { .. })
    ));
    assert!(matches!(
        http::parse_checksum_value(""),
        Err(InstallerError::EmptyChecksum)
    ));

    let sample_file = temp.path().join("sample_file.txt");
    fs::write(&sample_file, b"sample content").unwrap();

    let zeros64 = "0000000000000000000000000000000000000000000000000000000000000000";
    assert!(matches!(
        http::verify_sha256(&sample_file, zeros64),
        Err(InstallerError::ChecksumMismatch { .. })
    ));

    assert!(matches!(
        http::verify_sha512(&sample_file, "invalid_b64!"),
        Err(InstallerError::InvalidIntegrity(_))
    ));
    assert!(matches!(
        http::verify_sha512(&sample_file, "aGVsbG8="),
        Err(InstallerError::InvalidIntegrity(_))
    ));

    let dummy_b64_64 =
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
    assert!(matches!(
        http::verify_sha512(&sample_file, dummy_b64_64),
        Err(InstallerError::ChecksumMismatch { .. })
    ));

    let win_x64 = providers::node::node_target(Platform {
        os: OperatingSystem::Windows,
        arch: Architecture::X64,
        bun_cpu: BunCpu::Baseline,
    });
    assert_eq!(win_x64.archive_name, "win-x64");

    let win_arm64 = providers::node::node_target(Platform {
        os: OperatingSystem::Windows,
        arch: Architecture::Arm64,
        bun_cpu: BunCpu::Baseline,
    });
    assert_eq!(win_arm64.archive_name, "win-arm64");

    let mac_x64 = providers::node::node_target(Platform {
        os: OperatingSystem::MacOs,
        arch: Architecture::X64,
        bun_cpu: BunCpu::Baseline,
    });
    assert_eq!(mac_x64.archive_name, "darwin-x64");

    let mac_arm64 = providers::node::node_target(Platform {
        os: OperatingSystem::MacOs,
        arch: Architecture::Arm64,
        bun_cpu: BunCpu::Baseline,
    });
    assert_eq!(mac_arm64.archive_name, "darwin-arm64");

    let mut text_single = HashMap::new();
    text_single.insert(
        "https://registry.npmjs.org/pnpm/10.2.0".to_owned(),
        r#"{
            "version": "10.2.0",
            "dist": {
                "tarball": "https://registry.npmjs.org/pnpm/-/pnpm-10.2.0.tgz",
                "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
            },
            "engines": { "node": ">=18" }
        }"#.to_owned(),
    );
    let client_npm_single = Arc::new(FakeHttpClient {
        text: text_single,
        downloads: HashMap::new(),
        text_count: Mutex::new(0),
        download_count: Mutex::new(0),
    });
    let installer_npm = Installer::with_client(
        storage.clone(),
        Platform::current().unwrap(),
        client_npm_single,
    );
    let req_single: ToolRequest = "pnpm@10.2.0".parse().unwrap();
    let release_single = installer_npm.resolve_tool(&req_single).unwrap();
    assert_eq!(release_single.version, Version::new(10, 2, 0));

    let mut text_all = HashMap::new();
    text_all.insert(
        "https://registry.npmjs.org/pnpm".to_owned(),
        r#"{
            "dist-tags": { "latest": "10.2.0" },
            "versions": {
                "10.2.0": {
                    "version": "10.2.0",
                    "dist": {
                        "tarball": "https://registry.npmjs.org/pnpm/-/pnpm-10.2.0.tgz",
                        "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
                    },
                    "engines": { "node": ">=18" }
                }
            }
        }"#.to_owned(),
    );
    let client_npm_all = Arc::new(FakeHttpClient {
        text: text_all,
        downloads: HashMap::new(),
        text_count: Mutex::new(0),
        download_count: Mutex::new(0),
    });
    let installer_npm_all = Installer::with_client(
        storage.clone(),
        Platform::current().unwrap(),
        client_npm_all,
    );

    let req_range: ToolRequest = "pnpm@10.x".parse().unwrap();
    let release_range = installer_npm_all.resolve_tool(&req_range).unwrap();
    assert_eq!(release_range.version, Version::new(10, 2, 0));

    let bun_unsupported = providers::github::GithubRuntime::Bun.asset_name(Platform {
        os: OperatingSystem::Linux,
        arch: Architecture::X64,
        bun_cpu: BunCpu::Unsupported,
    });
    assert!(matches!(
        bun_unsupported,
        Err(InstallerError::UnsupportedBunCpu)
    ));

    let invalid_range = archive::validate_node_engine(
        jolter_runtime::ToolKind::Pnpm,
        &Version::new(10, 0, 0),
        Some("invalid_range_syntax!!!"),
        &Version::new(24, 0, 0),
    );
    assert!(matches!(
        invalid_range,
        Err(InstallerError::InvalidNodeEngineRange { .. })
    ));

    let incompatible = archive::validate_node_engine(
        jolter_runtime::ToolKind::Pnpm,
        &Version::new(10, 0, 0),
        Some("< 18.0.0"),
        &Version::new(24, 0, 0),
    );
    assert!(matches!(
        incompatible,
        Err(InstallerError::IncompatibleNodeVersion { .. })
    ));

    let v1 = Version::new(1, 0, 0);
    let tool_dir = storage.tool_version_dir(jolter_runtime::ToolKind::Pnpm, &v1);
    fs::create_dir_all(&tool_dir).unwrap();
    fs::write(tool_dir.join("dummy.txt"), b"dummy").unwrap();
    let rm_tool = installer
        .uninstall_tool(jolter_runtime::ToolKind::Pnpm, &v1)
        .unwrap();
    assert_eq!(rm_tool.path, tool_dir);

    let pt_dir = storage.plugin_tool_version_dir("scope", "eslint-cli", &v1);
    fs::create_dir_all(&pt_dir).unwrap();
    fs::write(pt_dir.join("dummy.txt"), b"dummy").unwrap();
    let rm_pt = installer
        .uninstall_plugin_tool("scope", "eslint-cli", &v1)
        .unwrap();
    assert_eq!(rm_pt.path, pt_dir);

    assert!(http::parse_github_digest("invalid_digest").is_none());
    assert!(http::parse_github_digest("sha256:short").is_none());

    let hash_file = temp.path().join("hash_sample.txt");
    fs::write(&hash_file, b"hash_sample").unwrap();

    let sha1_hash = jolter_runtime::ToolHash {
        algorithm: jolter_runtime::ToolHashAlgorithm::Sha1,
        value: "0000000000000000000000000000000000000000".to_owned(),
    };
    assert!(matches!(
        http::verify_tool_hash(&hash_file, &sha1_hash),
        Err(InstallerError::ToolHashMismatch { .. })
    ));

    let sha384_hash = jolter_runtime::ToolHash {
        algorithm: jolter_runtime::ToolHashAlgorithm::Sha384,
        value: "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000".to_owned(),
    };
    assert!(matches!(
        http::verify_tool_hash(&hash_file, &sha384_hash),
        Err(InstallerError::ToolHashMismatch { .. })
    ));

    assert_eq!(
        http::parse_github_digest(
            "sha256:0000000000000000000000000000000000000000000000000000000000000000"
        ),
        Some("0000000000000000000000000000000000000000000000000000000000000000".to_owned())
    );

    assert!(matches!(
        http::write_cache_file(Path::new(""), b"test"),
        Err(InstallerError::MetadataCacheRead { .. })
    ));

    let cache_dir = temp.path().join("cache_dir");
    fs::create_dir_all(&cache_dir).unwrap();
    let cache_dest = cache_dir.join("cache_file.txt");
    http::write_cache_file(&cache_dest, b"cached_content").unwrap();
    assert_eq!(fs::read(&cache_dest).unwrap(), b"cached_content");

    assert!(!http::offline_mode());

    let zip_path = temp.path().join("test_archive.zip");
    let zip_file = fs::File::create(&zip_path).unwrap();

    let mut zip_writer = zip::ZipWriter::new(zip_file);
    let options = zip::write::SimpleFileOptions::default();
    zip_writer.add_directory("dir/", options).unwrap();
    zip_writer.start_file("dir/test.txt", options).unwrap();
    zip_writer.write_all(b"zip_content").unwrap();
    zip_writer.finish().unwrap();

    let zip_dest = temp.path().join("zip_out");
    archive::extract_archive(&zip_path, &zip_dest, ArchiveFormat::Zip, 1).unwrap();
    assert_eq!(
        fs::read(&zip_dest.join("test.txt")).unwrap(),
        b"zip_content"
    );

    let empty_gh_release = providers::github::GithubRelease {
        tag_name: "v1.0.0".to_string(),
        draft: false,
        prerelease: false,
        assets: vec![],
    };
    let gh_asset = providers::github::GithubAsset {
        name: "deno-target.zip".to_string(),
        browser_download_url: "https://example.test/deno.zip".to_string(),
        digest: None,
    };
    assert!(matches!(
        providers::github::GithubRuntime::Deno.fallback_checksum(
            &installer,
            &empty_gh_release,
            &gh_asset,
            "deno-target.zip"
        ),
        Err(InstallerError::ChecksumAssetNotFound { .. })
    ));
    assert!(matches!(
        providers::github::GithubRuntime::Bun.fallback_checksum(
            &installer,
            &empty_gh_release,
            &gh_asset,
            "bun-target.zip"
        ),
        Err(InstallerError::ChecksumAssetNotFound { .. })
    ));

    let mut text_node_empty = HashMap::new();
    text_node_empty.insert(NODE_INDEX_URL.to_owned(), "[]".to_owned());
    let client_node_empty = Arc::new(FakeHttpClient {
        text: text_node_empty,
        downloads: HashMap::new(),
        text_count: Mutex::new(0),
        download_count: Mutex::new(0),
    });
    let installer_node_empty = Installer::with_client(
        storage.clone(),
        Platform::current().unwrap(),
        client_node_empty,
    );
    let req_node_missing =
        jolter_runtime::RuntimeRequest::new(jolter_runtime::RuntimeKind::Node, "99.x").unwrap();
    assert!(matches!(
        installer_node_empty.resolve(&req_node_missing),
        Err(InstallerError::VersionNotFound(_))
    ));

    let mut text_tool_empty = HashMap::new();
    text_tool_empty.insert(
        "https://registry.npmjs.org/pnpm".to_owned(),
        r#"{"dist-tags":{},"versions":{}}"#.to_owned(),
    );
    let client_tool_empty = Arc::new(FakeHttpClient {
        text: text_tool_empty,
        downloads: HashMap::new(),
        text_count: Mutex::new(0),
        download_count: Mutex::new(0),
    });
    let installer_tool_empty = Installer::with_client(
        storage.clone(),
        Platform::current().unwrap(),
        client_tool_empty,
    );
    let req_tool_missing: ToolRequest = "pnpm@99.x".parse().unwrap();
    assert!(matches!(
        installer_tool_empty.resolve_tool(&req_tool_missing),
        Err(InstallerError::ToolVersionNotFound(_))
    ));

    let mut text_bad_url = HashMap::new();
    text_bad_url.insert(
        "https://registry.npmjs.org/pnpm".to_owned(),
        r#"{"dist-tags":{"latest":"1.0.0"},"versions":{"1.0.0":{"version":"1.0.0","dist":{"tarball":"not_a_url","integrity":"sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="}}}}"#.to_owned(),
    );

    let client_bad_url = Arc::new(FakeHttpClient {
        text: text_bad_url,
        downloads: HashMap::new(),
        text_count: Mutex::new(0),
        download_count: Mutex::new(0),
    });
    let storage_bad = Storage::new(temp.path().join("storage_bad"));
    let installer_bad_url =
        Installer::with_client(storage_bad, Platform::current().unwrap(), client_bad_url);
    let req_bad_url: ToolRequest = "pnpm@1.0.x".parse().unwrap();
    assert!(matches!(
        installer_bad_url.resolve_tool(&req_bad_url),
        Err(InstallerError::InvalidUrl(_))
    ));

    assert!(matches!(
        http::ensure_https("http://example.test"),
        Err(InstallerError::InsecureUrl(_))
    ));

    for action in [
        progress::ProgressAction::Select,
        progress::ProgressAction::Resolve,
        progress::ProgressAction::Reuse,
        progress::ProgressAction::Connect,
        progress::ProgressAction::Download,
        progress::ProgressAction::Verify,
        progress::ProgressAction::Extract,
        progress::ProgressAction::Publish,
        progress::ProgressAction::Activate,
        progress::ProgressAction::Remove,
        progress::ProgressAction::Clean,
        progress::ProgressAction::Diagnose,
        progress::ProgressAction::Configure,
        progress::ProgressAction::Shims,
    ] {
        assert!(!action.label().is_empty());
    }

    let bad_pt_release = types::PluginToolArchive {
        provider: "provider".to_string(),
        tool: "tool".to_string(),
        version: Version::new(1, 0, 0),
        artifact: types::Artifact {
            url: "https://example.test/tool.zip".to_string(),
            integrity: types::ArtifactIntegrity::Sha512("sha512-hash".to_string()),
            file_name: "tool.zip".to_string(),
            format: types::ArchiveFormat::Zip,
            strip_components: 0,
        },
        commands: vec![],
    };
    assert!(matches!(
        installer::manifests::write_plugin_tool_manifest(temp.path(), &bad_pt_release),
        Err(InstallerError::UnsupportedIntegrity(_))
    ));
}
