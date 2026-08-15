use std::{
    env,
    path::Path,
    process::{Command, ExitCode},
};

use jolter_runtime::RuntimeKind;
use jolter_storage::Storage;

use crate::{error::ShimError, resolver::resolve_command};

#[must_use]
pub fn invoked_command_name() -> Option<String> {
    env::args_os()
        .next()
        .and_then(|argument| {
            Path::new(&argument)
                .file_stem()
                .map(|name| name.to_string_lossy().to_ascii_lowercase())
        })
        .filter(|name| !name.is_empty())
}

pub fn run_invoked_command() -> Result<ExitCode, ShimError> {
    let command = invoked_command_name()
        .filter(|name| name != "jolter-shim")
        .ok_or_else(|| ShimError::UnsupportedCommand("jolter-shim".to_owned()))?;
    run_command(&command)
}

pub fn run_command(command_name: &str) -> Result<ExitCode, ShimError> {
    let storage = Storage::discover()?;
    let current_dir = env::current_dir().map_err(ShimError::CurrentDirectory)?;
    let resolved = resolve_command(command_name, &current_dir, &storage)?;
    let mut command = Command::new(&resolved.executable);
    command.args(&resolved.arguments);
    command.args(env::args_os().skip(1));
    if let (Some(runtime), Some(runtime_root)) = (&resolved.runtime, &resolved.runtime_root) {
        prepend_runtime_path(&mut command, runtime.kind, runtime_root)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let error = command.exec();
        Err(ShimError::Launch {
            path: resolved.executable,
            source: error,
        })
    }
    #[cfg(not(unix))]
    {
        let status = command.status().map_err(|source| ShimError::Launch {
            path: resolved.executable,
            source,
        })?;
        Ok(status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .map_or(ExitCode::FAILURE, ExitCode::from))
    }
}

pub(crate) fn prepend_runtime_path(
    command: &mut Command,
    kind: RuntimeKind,
    runtime_root: &Path,
) -> Result<(), ShimError> {
    let binary_directory = if kind == RuntimeKind::Node && !cfg!(windows) {
        runtime_root.join("bin")
    } else {
        runtime_root.to_path_buf()
    };
    let mut paths = vec![binary_directory];
    if let Some(existing) = env::var_os("PATH") {
        paths.extend(env::split_paths(&existing));
    }
    let path = env::join_paths(paths).map_err(ShimError::JoinPath)?;
    command.env("PATH", path);
    command.env("JOLTER_RUNTIME_ROOT", runtime_root);
    Ok(())
}
