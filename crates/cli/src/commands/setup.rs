use std::{
    env,
    path::{Path, PathBuf},
};

use clap::CommandFactory;
use clap_complete::{
    generate,
    shells::{Bash, Elvish, Fish, PowerShell, Zsh},
};
use jolter_core::Jolter;

use crate::{
    args::{Cli, CompletionShell, SetupShell},
    error::CliError,
    output::TerminalUi,
};

pub fn install_shims(jolter: &Jolter) -> Result<(), CliError> {
    let executable = shim_executable()?;
    jolter.install_shims(&executable)?;
    Ok(())
}

pub fn shim_executable() -> Result<PathBuf, CliError> {
    if let Some(path) = env::var_os("JOLTER_SHIM_EXE").filter(|value| !value.is_empty()) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(CliError::ShimExecutableMissing(path));
    }
    let current = env::current_exe().map_err(CliError::CurrentExecutable)?;
    let directory = current
        .parent()
        .ok_or_else(|| CliError::ShimExecutableMissing(current.clone()))?;
    let executable = directory.join(if cfg!(windows) {
        "jolter-shim.exe"
    } else {
        "jolter-shim"
    });
    if executable.is_file() {
        Ok(executable)
    } else if current.is_file() {
        Ok(current)
    } else {
        Err(CliError::ShimExecutableMissing(executable))
    }
}

pub fn print_setup(jolter: &Jolter, shell: SetupShell, ui: &TerminalUi) {
    let shims = jolter.storage().shims_dir();
    ui.success(format!("Installed Jolter shims in {}", shims.display()));
    if path_contains(&shims) {
        ui.info("Jolter shims are already available on PATH in this process.");
        return;
    }

    ui.info("Add the shims directory to PATH:");
    match shell {
        SetupShell::Powershell => {
            let path = powershell_quote(&shims);
            ui.heading("Current PowerShell session");
            ui.line(format!("$env:PATH = '{path};' + $env:PATH"));
            ui.heading("Persist for the current user");
            ui.line(powershell_persist_command(&path));
        }
        SetupShell::Cmd => {
            let display = shims.display();
            let path = powershell_quote(&shims);
            ui.heading("Current Command Prompt session");
            ui.line(format!("set \"PATH={display};%PATH%\""));
            ui.heading("Persist for the current user");
            ui.line(format!(
                "powershell -NoProfile -Command \"{}\"",
                powershell_persist_script(&format!("'{path}'"))
            ));
        }
        SetupShell::Bash | SetupShell::Zsh => {
            let path = posix_double_quote_content(&shims);
            let export = format!("export PATH=\"{path}:$PATH\"");
            let profile = if shell == SetupShell::Zsh {
                "~/.zshrc"
            } else {
                "~/.bashrc"
            };
            ui.heading("Current shell session");
            ui.line(&export);
            ui.heading("Persist for future sessions");
            ui.line(format!(
                "printf '%s\\n' {} >> {profile}",
                posix_quote(&export)
            ));
        }
        SetupShell::Fish => {
            ui.heading("Current and future Fish sessions");
            ui.line(format!(
                "fish_add_path {}",
                posix_quote(&shims.to_string_lossy())
            ));
        }
        SetupShell::Auto => unreachable!("auto shell must be resolved before printing setup"),
    }
    ui.detail("Restart the shell after applying persistent PATH changes.");
}

pub fn resolve_setup_shell(shell: SetupShell) -> SetupShell {
    if shell != SetupShell::Auto {
        return shell;
    }
    if cfg!(windows) {
        return SetupShell::Powershell;
    }
    let detected = env::var_os("SHELL")
        .and_then(|value| {
            PathBuf::from(value)
                .file_stem()
                .map(std::borrow::ToOwned::to_owned)
        })
        .map(|name| name.to_string_lossy().to_ascii_lowercase());
    match detected.as_deref() {
        Some("zsh") => SetupShell::Zsh,
        Some("fish") => SetupShell::Fish,
        _ => SetupShell::Bash,
    }
}

pub fn path_contains(directory: &Path) -> bool {
    env::var_os("PATH")
        .is_some_and(|value| env::split_paths(&value).any(|entry| same_path(&entry, directory)))
}

pub fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

pub fn powershell_quote(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}

pub fn powershell_persist_command(path: &str) -> String {
    format!(
        "$jolter = '{path}'; {}",
        powershell_persist_script("$jolter")
    )
}

pub fn powershell_persist_script(path_expression: &str) -> String {
    format!(
        "$userPath = [Environment]::GetEnvironmentVariable('Path', 'User'); \
if (-not $userPath) {{ $userPath = '' }}; \
if (($userPath -split ';') -notcontains {path_expression}) {{ \
[Environment]::SetEnvironmentVariable('Path', \
(($userPath.TrimEnd(';') + ';' + {path_expression}).Trim(';')), 'User') }}"
    )
}

pub fn posix_double_quote_content(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

pub fn posix_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn print_completions(shell: CompletionShell) {
    let mut command = Cli::command();
    let name = command.get_name().to_owned();
    match shell {
        CompletionShell::Bash => generate(Bash, &mut command, name, &mut std::io::stdout()),
        CompletionShell::Elvish => generate(Elvish, &mut command, name, &mut std::io::stdout()),
        CompletionShell::Fish => generate(Fish, &mut command, name, &mut std::io::stdout()),
        CompletionShell::Powershell => {
            generate(PowerShell, &mut command, name, &mut std::io::stdout());
        }
        CompletionShell::Zsh => generate(Zsh, &mut command, name, &mut std::io::stdout()),
    }
}
