use clap::{Parser, Subcommand, ValueEnum};
use jolter_core::ReleaseChannel;
use jolter_plugin::PluginRequest;
use jolter_runtime::{RuntimeKind, RuntimeRequest, ToolKind, ToolRequest};
use semver::Version;

#[derive(Debug, Parser)]
#[command(name = "jolter", version, about = "JavaScript toolchain manager")]
#[allow(clippy::struct_excessive_bools)]
pub struct Cli {
    /// Disable the live updating progress line.
    #[arg(long, global = true)]
    pub no_progress: bool,
    /// Disable ANSI colors.
    #[arg(long, global = true)]
    pub no_color: bool,
    /// Suppress operational progress while retaining command results.
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,
    /// Include transfer timing and additional operational detail.
    #[arg(short, long, global = true, conflicts_with = "quiet")]
    pub verbose: bool,
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn machine_output(&self) -> bool {
        matches!(
            &self.command,
            Command::List { json: true }
                | Command::Doctor { json: true }
                | Command::SetupCi { json: true, .. }
                | Command::Plugin {
                    command: PluginCommand::List { json: true },
                }
                | Command::Completions { .. }
        )
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Install shims and print shell-specific PATH setup commands.
    Setup {
        /// Shell to configure. Auto detects the current shell where possible.
        #[arg(long, value_enum, default_value_t = SetupShell::Auto)]
        shell: SetupShell,
    },
    /// Download and install a runtime or tool without setting it as active.
    #[command(visible_alias = "i")]
    Install {
        /// Tool request, for example node@24 or pnpm@10.
        #[arg(value_parser = parse_use_target, num_args = 0..)]
        target: Vec<UseTarget>,
    },
    /// Install and activate a runtime or tool.
    Use {
        /// Tool request, for example node@24 or pnpm@10.
        #[arg(value_parser = parse_use_target, num_args = 0..)]
        target: Vec<UseTarget>,
    },
    /// Write a runtime or tool requirement to jolter.json.
    Pin {
        /// Runtime or tool request, for example node@24 or pnpm@10.
        #[arg(value_parser = parse_use_target, num_args = 1..)]
        target: Vec<UseTarget>,
    },
    /// Update an active runtime or tool.
    #[command(visible_alias = "up")]
    Update {
        /// Runtime or tool name, optionally with a selector.
        #[arg(
            value_parser = parse_update_target,
            required_unless_present = "all",
            conflicts_with = "all"
        )]
        target: Option<UpdateTarget>,
        /// Update every active runtime and tool within its current major line.
        #[arg(long)]
        all: bool,
    },
    /// List locally installed runtimes and tools.
    #[command(visible_alias = "ls")]
    List {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Check project toolchain health.
    Doctor {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Repair detected toolchain problems.
    Repair {
        /// Install missing project plugins without prompting.
        #[arg(long)]
        yes: bool,
    },
    /// Synchronize the local toolchain with project requirements.
    Sync {
        /// Install missing project plugins without prompting.
        #[arg(long)]
        yes: bool,
    },
    /// Install, update, list, or remove Jolter plugins.
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
    },
    /// Remove one exact runtime or tool version.
    #[command(visible_aliases = ["remove", "rm"])]
    Uninstall {
        /// Exact version to remove, for example node@24.1.0 or pnpm@10.2.0.
        #[arg(value_parser = parse_uninstall_target)]
        target: UninstallTarget,
        /// Permit removal when the runtime or tool is globally active.
        #[arg(long)]
        force: bool,
    },
    /// Remove old and incomplete installations.
    Prune {
        /// Number of newest complete versions to keep for each tool.
        #[arg(long, default_value_t = 1)]
        keep: usize,
        /// Print what would be removed without changing storage.
        #[arg(long)]
        dry_run: bool,
    },
    /// Inspect or clean Jolter's download and metadata cache.
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
    /// Synchronize the project and expose its exact toolchain to CI.
    SetupCi {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
        /// Install missing project plugins without prompting.
        #[arg(long)]
        yes: bool,
    },
    /// Generate shell completion scripts.
    #[command(visible_aliases = ["c", "comp"])]
    Completions {
        /// Shell whose completion script should be generated.
        #[arg(value_enum)]
        shell: CompletionShell,
    },
    /// Upgrade Jolter executable to the latest release.
    Upgrade {
        /// Switch to the nightly channel and install the latest nightly build.
        #[arg(long, conflicts_with = "latest")]
        nightly: bool,
        /// Switch to the latest channel and install latest build.
        #[arg(long, conflicts_with = "nightly")]
        latest: bool,
        /// Release channel to use (stable or nightly).
        #[arg(long, value_enum, default_value_t = ChannelArg::Stable, conflicts_with_all = ["nightly", "latest"])]
        channel: ChannelArg,
        /// Force re-installation even if already on the requested version.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ChannelArg {
    Stable,
    Nightly,
}

impl From<ChannelArg> for ReleaseChannel {
    fn from(arg: ChannelArg) -> Self {
        match arg {
            ChannelArg::Stable => Self::Stable,
            ChannelArg::Nightly => Self::Nightly,
        }
    }
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub enum CacheCommand {
    /// Report the number and size of cached files.
    Status,
    /// Remove cached downloads and release metadata.
    Clean,
}

#[derive(Debug, Clone, Subcommand)]
pub enum PluginCommand {
    /// Install a plugin globally.
    #[command(visible_aliases = ["add", "i"])]
    Install {
        /// Plugin request, for example eslint, eslint@1, or @eslint/eslint@1.
        #[arg(value_parser = parse_plugin_request)]
        target: PluginRequest,
    },
    /// List installed plugins.
    #[command(visible_alias = "ls")]
    List {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Update installed plugins.
    #[command(visible_aliases = ["up"])]
    Update {
        /// Plugin name or alias to update.
        target: Option<String>,
        /// Update every installed plugin.
        #[arg(long)]
        all: bool,
    },
    /// Remove an installed plugin.
    #[command(visible_aliases = ["remove", "rm"])]
    Uninstall {
        /// Plugin name or alias.
        name: String,
        /// Permit removal when plugin commands are shimmed.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    Powershell,
    Zsh,
}

#[derive(Debug, Clone)]
pub enum UninstallTarget {
    Runtime(RuntimeKind, Version),
    Tool(ToolKind, Version),
    PluginTool { name: String, version: Version },
}

impl std::fmt::Display for UninstallTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runtime(kind, version) => write!(f, "{kind}@{version}"),
            Self::Tool(kind, version) => write!(f, "{kind}@{version}"),
            Self::PluginTool { name, version } => write!(f, "{name}@{version}"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum UseTarget {
    Runtime(RuntimeRequest),
    Tool(ToolRequest),
    PluginTool { name: String, selector: String },
}

impl std::fmt::Display for UseTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runtime(request) => write!(f, "{request}"),
            Self::Tool(request) => write!(f, "{request}"),
            Self::PluginTool { name, selector } => write!(f, "{name}@{selector}"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum UpdateTarget {
    Runtime(RuntimeKind, Option<RuntimeRequest>),
    Tool(ToolKind, Option<ToolRequest>),
    PluginTool {
        name: String,
        selector: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SetupShell {
    Auto,
    Powershell,
    Cmd,
    Bash,
    Zsh,
    Fish,
}

fn is_valid_tool_identifier(name: &str) -> bool {
    let name = name.strip_prefix('@').unwrap_or(name);
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '/' || c == '.')
}

pub fn parse_use_target(input: &str) -> Result<UseTarget, String> {
    if let Ok(request) = input.parse::<RuntimeRequest>() {
        return Ok(UseTarget::Runtime(request));
    }
    if let Ok(request) = input.parse::<ToolRequest>() {
        return Ok(UseTarget::Tool(request));
    }
    if let Some((name, selector)) = input.split_once('@') {
        if is_valid_tool_identifier(name) && !selector.is_empty() {
            return Ok(UseTarget::PluginTool {
                name: name.to_owned(),
                selector: selector.to_owned(),
            });
        }
    } else if is_valid_tool_identifier(input) {
        return Ok(UseTarget::PluginTool {
            name: input.to_owned(),
            selector: "latest".to_owned(),
        });
    }
    Err(format!("invalid target `{input}`"))
}

pub fn parse_update_target(input: &str) -> Result<UpdateTarget, String> {
    let (name, selector) = input.split_once('@').unwrap_or((input, ""));
    let selector_opt = if selector.is_empty() {
        None
    } else {
        Some(selector)
    };
    if let Ok(kind) = name.parse::<RuntimeKind>() {
        let request = selector_opt
            .map(|s| format!("{kind}@{s}").parse::<RuntimeRequest>())
            .transpose()
            .map_err(|e| e.to_string())?;
        return Ok(UpdateTarget::Runtime(kind, request));
    }
    if let Ok(kind) = name.parse::<ToolKind>() {
        let request = selector_opt
            .map(|s| format!("{kind}@{s}").parse::<ToolRequest>())
            .transpose()
            .map_err(|e| e.to_string())?;
        return Ok(UpdateTarget::Tool(kind, request));
    }
    if is_valid_tool_identifier(name) {
        return Ok(UpdateTarget::PluginTool {
            name: name.to_owned(),
            selector: selector_opt.map(ToOwned::to_owned),
        });
    }
    Err(format!("invalid update target `{input}`"))
}

pub fn parse_uninstall_target(input: &str) -> Result<UninstallTarget, String> {
    let (name, version_str) = input
        .split_once('@')
        .ok_or_else(|| format!("target `{input}` must specify an exact version"))?;
    let version = Version::parse(version_str)
        .map_err(|_| format!("invalid version `{version_str}` in `{input}`"))?;
    if let Ok(kind) = name.parse::<RuntimeKind>() {
        return Ok(UninstallTarget::Runtime(kind, version));
    }
    if let Ok(kind) = name.parse::<ToolKind>() {
        return Ok(UninstallTarget::Tool(kind, version));
    }
    if is_valid_tool_identifier(name) {
        return Ok(UninstallTarget::PluginTool {
            name: name.to_owned(),
            version,
        });
    }
    Err(format!("invalid uninstall target `{input}`"))
}

pub fn parse_plugin_request(input: &str) -> Result<PluginRequest, String> {
    input
        .parse::<PluginRequest>()
        .map_err(|err| err.to_string())
}
