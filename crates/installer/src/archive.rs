use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Component, Path, PathBuf},
};

use flate2::read::GzDecoder;
use jolter_runtime::ToolKind;
use nodejs_semver::{Range as NodeRange, Version as NodeVersion};
use semver::Version;

use crate::{MAX_ARCHIVE_BYTES, MAX_ARCHIVE_ENTRIES, error::InstallerError, types::ArchiveFormat};

pub(crate) fn validate_node_engine(
    kind: ToolKind,
    tool_version: &Version,
    requirement: Option<&str>,
    node_version: &Version,
) -> Result<(), InstallerError> {
    let Some(requirement) = requirement.filter(|value| !value.trim().is_empty()) else {
        return Ok(());
    };
    let range =
        NodeRange::parse(requirement).map_err(|source| InstallerError::InvalidNodeEngineRange {
            tool: kind,
            version: tool_version.clone(),
            requirement: requirement.to_owned(),
            details: source.to_string(),
        })?;
    let node = NodeVersion::from((node_version.major, node_version.minor, node_version.patch));
    if range.satisfies(&node) {
        Ok(())
    } else {
        Err(InstallerError::IncompatibleNodeVersion {
            tool: kind,
            version: tool_version.clone(),
            requirement: requirement.to_owned(),
            node_version: node_version.clone(),
        })
    }
}

pub(crate) fn extract_archive(
    archive: &Path,
    destination: &Path,
    format: ArchiveFormat,
    strip_components: usize,
) -> Result<(), InstallerError> {
    match format {
        ArchiveFormat::Zip => extract_zip(archive, destination, strip_components),
        ArchiveFormat::TarGz => extract_tar_gz(archive, destination, strip_components),
    }
}

fn extract_zip(
    archive: &Path,
    destination: &Path,
    strip_components: usize,
) -> Result<(), InstallerError> {
    let file = File::open(archive).map_err(InstallerError::Io)?;
    let reader = BufReader::with_capacity(128 * 1024, file);
    let mut zip = zip::ZipArchive::new(reader).map_err(InstallerError::Zip)?;
    if zip.len() > MAX_ARCHIVE_ENTRIES {
        return Err(InstallerError::ArchiveEntryLimit);
    }
    let mut created_dirs: HashSet<PathBuf> = HashSet::new();
    let mut extracted = 0_u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(InstallerError::Zip)?;
        if entry.is_symlink() {
            return Err(InstallerError::UnsupportedArchiveEntry(
                entry.name().to_owned(),
            ));
        }
        extracted = extracted
            .checked_add(entry.size())
            .ok_or(InstallerError::ArchiveSizeLimit)?;
        if extracted > MAX_ARCHIVE_BYTES {
            return Err(InstallerError::ArchiveSizeLimit);
        }
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| InstallerError::UnsafeArchivePath(entry.name().to_owned()))?;
        let Some(relative) = strip_leading_components(&enclosed, strip_components)? else {
            continue;
        };
        let output = destination.join(relative);
        ensure_safe_parent(destination, &output)?;
        if entry.is_dir() {
            if created_dirs.insert(output.clone()) {
                fs::create_dir_all(&output).map_err(InstallerError::Io)?;
            }
            continue;
        }
        if let Some(parent) = output.parent() {
            if created_dirs.insert(parent.to_path_buf()) {
                fs::create_dir_all(parent).map_err(InstallerError::Io)?;
            }
        }
        let output_file = File::create(&output).map_err(InstallerError::Io)?;
        let mut writer = BufWriter::with_capacity(64 * 1024, output_file);
        io::copy(&mut entry, &mut writer).map_err(InstallerError::Io)?;
        writer.flush().map_err(InstallerError::Io)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            set_mode(&output, mode)?;
        }
    }
    Ok(())
}

fn extract_tar_gz(
    archive: &Path,
    destination: &Path,
    strip_components: usize,
) -> Result<(), InstallerError> {
    let file = File::open(archive).map_err(InstallerError::Io)?;
    let reader = BufReader::with_capacity(128 * 1024, file);
    let decoder = GzDecoder::new(reader);
    let mut tar = tar::Archive::new(decoder);
    let mut created_dirs: HashSet<PathBuf> = HashSet::new();
    let mut extracted = 0_u64;
    let mut entries = 0_usize;
    for entry in tar.entries().map_err(InstallerError::Io)? {
        entries += 1;
        if entries > MAX_ARCHIVE_ENTRIES {
            return Err(InstallerError::ArchiveEntryLimit);
        }
        let mut entry = entry.map_err(InstallerError::Io)?;
        let path = entry.path().map_err(InstallerError::Io)?.into_owned();
        let Some(relative) = strip_leading_components(&path, strip_components)? else {
            continue;
        };
        let output = destination.join(&relative);
        ensure_safe_parent(destination, &output)?;
        let entry_type = entry.header().entry_type();

        if entry_type.is_dir() {
            if created_dirs.insert(output.clone()) {
                fs::create_dir_all(&output).map_err(InstallerError::Io)?;
            }
        } else if entry_type.is_file() {
            let size = entry.header().size().map_err(InstallerError::Io)?;
            extracted = extracted
                .checked_add(size)
                .ok_or(InstallerError::ArchiveSizeLimit)?;
            if extracted > MAX_ARCHIVE_BYTES {
                return Err(InstallerError::ArchiveSizeLimit);
            }
            if let Some(parent) = output.parent() {
                if created_dirs.insert(parent.to_path_buf()) {
                    fs::create_dir_all(parent).map_err(InstallerError::Io)?;
                }
            }
            let output_file = File::create(&output).map_err(InstallerError::Io)?;
            let mut writer = BufWriter::with_capacity(64 * 1024, output_file);
            io::copy(&mut entry, &mut writer).map_err(InstallerError::Io)?;
            writer.flush().map_err(InstallerError::Io)?;
            #[cfg(unix)]
            set_mode(&output, entry.header().mode().map_err(InstallerError::Io)?)?;
        } else if entry_type.is_symlink() {
            extract_symlink(&entry, destination, &relative, &output)?;
        } else if entry_type.is_hard_link() {
            extract_hard_link(&entry, destination, strip_components, &output)?;
        } else {
            return Err(InstallerError::UnsupportedArchiveEntry(
                path.display().to_string(),
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn extract_symlink<R: Read>(
    entry: &tar::Entry<'_, R>,
    destination: &Path,
    relative: &Path,
    output: &Path,
) -> Result<(), InstallerError> {
    let target = entry
        .link_name()
        .map_err(InstallerError::Io)?
        .ok_or_else(|| InstallerError::UnsafeArchivePath(relative.display().to_string()))?;
    validate_symlink_target(relative, &target)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(InstallerError::Io)?;
        ensure_safe_parent(destination, output)?;
    }
    std::os::unix::fs::symlink(target, output).map_err(InstallerError::Io)
}

#[cfg(not(unix))]
fn extract_symlink<R: Read>(
    _entry: &tar::Entry<'_, R>,
    _destination: &Path,
    relative: &Path,
    _output: &Path,
) -> Result<(), InstallerError> {
    Err(InstallerError::UnsupportedArchiveEntry(
        relative.display().to_string(),
    ))
}

#[cfg(unix)]
fn extract_hard_link<R: Read>(
    entry: &tar::Entry<'_, R>,
    destination: &Path,
    strip_components: usize,
    output: &Path,
) -> Result<(), InstallerError> {
    let target = entry
        .link_name()
        .map_err(InstallerError::Io)?
        .ok_or_else(|| InstallerError::UnsafeArchivePath(output.display().to_string()))?;
    let target = strip_leading_components(&target, strip_components)?
        .ok_or_else(|| InstallerError::UnsafeArchivePath(target.display().to_string()))?;
    let source = destination.join(target);
    ensure_safe_parent(destination, &source)?;
    if !source.is_file() {
        return Err(InstallerError::UnsafeArchivePath(
            source.display().to_string(),
        ));
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(InstallerError::Io)?;
        ensure_safe_parent(destination, output)?;
    }
    fs::hard_link(source, output).map_err(InstallerError::Io)
}

#[cfg(not(unix))]
fn extract_hard_link<R: Read>(
    _entry: &tar::Entry<'_, R>,
    _destination: &Path,
    _strip_components: usize,
    relative: &Path,
) -> Result<(), InstallerError> {
    Err(InstallerError::UnsupportedArchiveEntry(
        relative.display().to_string(),
    ))
}

pub(crate) fn strip_leading_components(
    path: &Path,
    strip_components: usize,
) -> Result<Option<PathBuf>, InstallerError> {
    let components = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part),
            Component::CurDir => None,
            _ => Some(std::ffi::OsStr::new("..")),
        })
        .collect::<Vec<_>>();
    if components.iter().any(|part| *part == "..") {
        return Err(InstallerError::UnsafeArchivePath(
            path.display().to_string(),
        ));
    }
    if components.len() <= strip_components {
        return Ok(None);
    }
    let mut relative = PathBuf::new();
    for part in &components[strip_components..] {
        relative.push(part);
    }
    Ok(Some(relative))
}

pub(crate) fn ensure_safe_parent(base: &Path, target: &Path) -> Result<(), InstallerError> {
    if target.starts_with(base) {
        Ok(())
    } else {
        Err(InstallerError::UnsafeArchivePath(
            target.display().to_string(),
        ))
    }
}

#[cfg(unix)]
pub(crate) fn validate_symlink_target(
    relative: &Path,
    target: &Path,
) -> Result<(), InstallerError> {
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let mut normalized = parent.to_path_buf();
    for component in target.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(InstallerError::UnsafeArchivePath(
                        target.display().to_string(),
                    ));
                }
            }
            Component::Normal(part) => normalized.push(part),
            Component::Prefix(_) | Component::RootDir => {
                return Err(InstallerError::UnsafeArchivePath(
                    target.display().to_string(),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), InstallerError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(InstallerError::Io)
}
