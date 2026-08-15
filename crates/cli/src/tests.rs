use super::*;
use clap::CommandFactory;
use std::fs;
use std::io::IsTerminal;

#[allow(clippy::wildcard_imports)]
use crate::args::*;

use jolter_runtime::{RuntimeKind, ToolKind};
use semver::Version;

use commands::{
    inventory::{print_inventory, print_inventory_json, print_plugins, print_plugins_json},
    use_cmd::interactive_use_target,
};

#[test]
fn validates_clap_cli_structure() {
    Cli::command().debug_assert();
}

#[test]
fn tests_use_target_parsing() {
    assert!(matches!(
        parse_use_target("node@20.0.0"),
        Ok(UseTarget::Runtime(_))
    ));
    assert!(matches!(
        parse_use_target("pnpm@10"),
        Ok(UseTarget::Tool(_))
    ));
    assert!(matches!(
        parse_use_target("my-tool@1.x"),
        Ok(UseTarget::PluginTool { .. })
    ));
    assert!(parse_use_target("invalid name!@1.0").is_err());
}

#[test]
fn tests_update_and_uninstall_target_parsing() {
    assert!(matches!(
        parse_update_target("node"),
        Ok(UpdateTarget::Runtime(RuntimeKind::Node, None))
    ));
    assert!(matches!(
        parse_update_target("pnpm@10.2.0"),
        Ok(UpdateTarget::Tool(ToolKind::Pnpm, Some(_)))
    ));

    assert!(matches!(
        parse_uninstall_target("node@24.1.0"),
        Ok(UninstallTarget::Runtime(RuntimeKind::Node, _))
    ));
    assert!(matches!(
        parse_uninstall_target("pnpm@10.2.0"),
        Ok(UninstallTarget::Tool(ToolKind::Pnpm, _))
    ));
    assert!(parse_uninstall_target("node").is_err());
}

#[test]
fn tests_cli_error_display() {
    let err = CliError::NoActiveUpdateTarget("node".to_owned());
    assert_eq!(
        err.to_string(),
        "no active node version; pass an explicit selector such as node@latest"
    );

    let err = CliError::PluginUpdateTargetRequired;
    assert_eq!(
        err.to_string(),
        "pass a plugin name or use `jolter plugin update --all`"
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn tests_run_cli_handlers() {
    let temp_dir = tempfile::tempdir().unwrap();
    let home_dir = temp_dir.path().join("home");
    let storage = jolter_storage::Storage::new(&home_dir);
    storage.ensure_layout().unwrap();

    fs::write(
        temp_dir.path().join("jolter.json"),
        r#"{"runtime":{"node":"24"}}"#,
    )
    .unwrap();

    let jolter = Jolter::with_storage(storage).unwrap();
    let ui = Arc::new(TerminalUi::new(OutputOptions {
        progress: ProgressPreference::Plain,
        color: ColorPreference::Never,
        detail: DetailLevel::Normal,
        kind: OutputKind::Human,
    }));

    let node_ver = semver::Version::new(24, 1, 0);
    let node_exe = jolter
        .storage()
        .runtime_executable(RuntimeKind::Node, &node_ver);
    fs::create_dir_all(node_exe.parent().unwrap()).unwrap();
    fs::write(&node_exe, b"node").unwrap();
    jolter
        .storage()
        .activate(RuntimeKind::Node, &node_ver)
        .unwrap();

    let pnpm_ver = semver::Version::new(10, 2, 0);
    let pnpm_ep = jolter
        .storage()
        .tool_entrypoint(ToolKind::Pnpm, &pnpm_ver, "pnpm")
        .unwrap();
    fs::create_dir_all(pnpm_ep.parent().unwrap()).unwrap();
    fs::write(&pnpm_ep, b"pnpm").unwrap();
    jolter
        .storage()
        .activate_tool(ToolKind::Pnpm, &pnpm_ver)
        .unwrap();

    assert!(print_inventory(&jolter, &ui).is_ok());
    assert!(print_inventory_json(&jolter).is_ok());
    assert!(print_plugins(&jolter, &ui).is_ok());
    assert!(print_plugins_json(&jolter).is_ok());
    print_completions(CompletionShell::Bash);
    print_completions(CompletionShell::Fish);
    print_completions(CompletionShell::Zsh);
    print_setup(&jolter, SetupShell::Bash, &ui);

    let _ = run_use(
        &jolter,
        vec![UseTarget::Runtime("node@24.1.0".parse().unwrap())],
        &ui,
    );
    let _ = run_use(
        &jolter,
        vec![UseTarget::Tool("pnpm@10.2.0".parse().unwrap())],
        &ui,
    );
    let _ = run_update(
        &jolter,
        Some(UpdateTarget::Runtime(RuntimeKind::Node, None)),
        false,
        &ui,
    );
    let _ = run_update(
        &jolter,
        Some(UpdateTarget::Tool(ToolKind::Pnpm, None)),
        false,
        &ui,
    );
    let _ = run_update(&jolter, None, true, &ui);
    assert!(run_prune(&jolter, temp_dir.path(), 1, true, &ui).is_ok());
    assert!(run_cache(&jolter, CacheCommand::Status, &ui).is_ok());
    assert!(run_cache(&jolter, CacheCommand::Clean, &ui).is_ok());
    let _ = run_setup_ci(&jolter, temp_dir.path(), false, false, &ui);
    let _ = run_setup_ci(&jolter, temp_dir.path(), true, false, &ui);
    assert!(run_doctor(&jolter, temp_dir.path(), false, &ui).is_ok());
    assert!(run_doctor(&jolter, temp_dir.path(), true, &ui).is_ok());

    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        let _ = run_use(&jolter, vec![], &ui);
    }

    let _ = run_update(
        &jolter,
        Some(UpdateTarget::PluginTool {
            name: "my-tool".to_owned(),
            selector: None,
        }),
        false,
        &ui,
    );
    let _ = run_uninstall(
        &jolter,
        UninstallTarget::PluginTool {
            name: "my-tool".to_owned(),
            version: Version::new(1, 0, 0),
        },
        true,
        &ui,
    );
    let _ = run_uninstall(
        &jolter,
        UninstallTarget::Tool(ToolKind::Pnpm, pnpm_ver),
        true,
        &ui,
    );
    let _ = run_uninstall(
        &jolter,
        UninstallTarget::Runtime(RuntimeKind::Node, node_ver),
        true,
        &ui,
    );
}

#[test]
fn tests_interactive_use_non_tty_error() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = jolter_storage::Storage::new(temp_dir.path());
    storage.ensure_layout().unwrap();
    let jolter = Jolter::with_storage(storage).unwrap();
    let ui = Arc::new(TerminalUi::new(OutputOptions {
        progress: ProgressPreference::Plain,
        color: ColorPreference::Never,
        detail: DetailLevel::Normal,
        kind: OutputKind::Human,
    }));
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        let err = interactive_use_target(&jolter, &ui).unwrap_err();
        assert!(matches!(err, CliError::InteractiveUseRequiresTty));
    }
}

#[test]
fn tests_run_dispatch_subcommands() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = jolter_storage::Storage::new(temp_dir.path());
    storage.ensure_layout().unwrap();
    let jolter = Jolter::with_storage(storage.clone()).unwrap();

    fs::write(
        temp_dir.path().join("jolter.json"),
        r#"{"runtime":{"node":"24.1.0"}}"#,
    )
    .unwrap();

    let node_ver = semver::Version::new(24, 1, 0);
    let node_exe = storage.runtime_executable(RuntimeKind::Node, &node_ver);
    fs::create_dir_all(node_exe.parent().unwrap()).unwrap();
    fs::write(&node_exe, b"node").unwrap();
    storage.activate(RuntimeKind::Node, &node_ver).unwrap();

    let ui = Arc::new(TerminalUi::new(OutputOptions {
        progress: ProgressPreference::Plain,
        color: ColorPreference::Never,
        detail: DetailLevel::Normal,
        kind: OutputKind::Human,
    }));

    let parse_and_run = |args: &[&str]| {
        let mut full_args = vec!["jolter"];
        full_args.extend(args);
        let cli = Cli::try_parse_from(full_args).unwrap();
        run_with_jolter_in_dir(&jolter, cli, temp_dir.path(), &ui)
    };

    assert!(parse_and_run(&["pin", "node@24.1.0"]).is_ok());
    assert!(parse_and_run(&["install", "node@24.1.0"]).is_ok());
    assert!(parse_and_run(&["i", "node@24.1.0"]).is_ok());
    assert!(parse_and_run(&["use", "node@24.1.0"]).is_ok());
    assert!(parse_and_run(&["update", "node@24.1.0"]).is_ok());
    assert!(parse_and_run(&["list"]).is_ok());
    assert!(parse_and_run(&["list", "--json"]).is_ok());
    assert!(parse_and_run(&["doctor"]).is_ok());
    assert!(parse_and_run(&["doctor", "--json"]).is_ok());
    assert!(parse_and_run(&["setup", "--shell", "bash"]).is_ok());
    assert!(parse_and_run(&["setup-ci"]).is_ok());
    assert!(parse_and_run(&["setup-ci", "--json"]).is_ok());
    assert!(parse_and_run(&["sync"]).is_ok());
    assert!(parse_and_run(&["prune"]).is_ok());
    assert!(parse_and_run(&["repair"]).is_ok());
    assert!(parse_and_run(&["cache", "status"]).is_ok());
    assert!(parse_and_run(&["cache", "clean"]).is_ok());
    assert!(parse_and_run(&["plugin", "list"]).is_ok());
    assert!(parse_and_run(&["plugin", "list", "--json"]).is_ok());
    assert!(parse_and_run(&["completions", "bash"]).is_ok());
    assert!(parse_and_run(&["completions", "elvish"]).is_ok());
    assert!(parse_and_run(&["completions", "fish"]).is_ok());
    assert!(parse_and_run(&["completions", "powershell"]).is_ok());
    assert!(parse_and_run(&["completions", "zsh"]).is_ok());
    assert!(parse_and_run(&["uninstall", "node@24.1.0", "--force"]).is_ok());
}

#[test]
fn tests_channel_arg_conversion() {
    let stable: jolter_core::ReleaseChannel = ChannelArg::Stable.into();
    assert_eq!(stable, jolter_core::ReleaseChannel::Stable);
    let nightly: jolter_core::ReleaseChannel = ChannelArg::Nightly.into();
    assert_eq!(nightly, jolter_core::ReleaseChannel::Nightly);
}

#[test]
fn tests_cli_target_parsers_and_error_paths() {
    assert!(parse_use_target("node@24").is_ok());
    assert!(parse_use_target("pnpm@10").is_ok());
    assert!(parse_use_target("my-tool@1.0").is_ok());
    assert!(parse_use_target("invalid target syntax!").is_err());

    assert!(parse_update_target("node@24").is_ok());
    assert!(parse_update_target("pnpm@10").is_ok());
    assert!(parse_update_target("my-tool@1.0").is_ok());
    assert!(parse_update_target("invalid update target!").is_err());

    assert!(parse_uninstall_target("node@24.1.0").is_ok());
    assert!(parse_uninstall_target("pnpm@10.2.0").is_ok());
    assert!(parse_uninstall_target("my-tool@1.0.0").is_ok());
    assert!(parse_uninstall_target("invalid uninstall target!").is_err());
}

#[test]
fn tests_plugin_subcommand_dispatch() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = jolter_storage::Storage::new(temp_dir.path());
    storage.ensure_layout().unwrap();
    let jolter = Jolter::with_storage(storage).unwrap();
    let ui = Arc::new(TerminalUi::new(OutputOptions {
        progress: ProgressPreference::Plain,
        color: ColorPreference::Never,
        detail: DetailLevel::Normal,
        kind: OutputKind::Human,
    }));

    let v1 = semver::Version::new(1, 0, 0);
    let v24 = semver::Version::new(24, 0, 0);
    let v10 = semver::Version::new(10, 0, 0);

    let node_exe = jolter
        .storage()
        .runtime_executable(jolter_runtime::RuntimeKind::Node, &v24);
    fs::create_dir_all(node_exe.parent().unwrap()).unwrap();
    fs::write(&node_exe, b"node").unwrap();
    jolter
        .storage()
        .activate(jolter_runtime::RuntimeKind::Node, &v24)
        .unwrap();

    let pnpm_exe = jolter.storage().node_tool_executable(&v24, "pnpm");
    fs::create_dir_all(pnpm_exe.parent().unwrap()).unwrap();
    fs::write(&pnpm_exe, b"pnpm").unwrap();
    let pnpm_ver_dir = jolter
        .storage()
        .tool_version_dir(jolter_runtime::ToolKind::Pnpm, &v10);
    fs::create_dir_all(&pnpm_ver_dir).unwrap();
    fs::write(pnpm_ver_dir.join("pnpm"), b"pnpm").unwrap();
    jolter
        .storage()
        .activate_tool(jolter_runtime::ToolKind::Pnpm, &v10)
        .unwrap();

    let plugin_dir = jolter.storage().plugin_version_dir("@scope/eslint", &v1);
    fs::create_dir_all(&plugin_dir).unwrap();
    fs::write(
        plugin_dir.join(".jolter-plugin.json"),
        r#"{"canonicalName":"@scope/eslint","requestedName":"eslint","version":"1.0.0","registryUrl":"","wasmSha256":"","commands":["eslint"],"provides":{"tools":{}}}"#,
    )
    .unwrap();

    let pt_dir = jolter
        .storage()
        .plugin_tool_version_dir("@scope/eslint", "eslint-cli", &v1);
    fs::create_dir_all(&pt_dir).unwrap();
    fs::write(
        pt_dir.join(".jolter-plugin-tool.json"),
        r#"{"commands":["eslint-cli"]}"#,
    )
    .unwrap();
    fs::write(pt_dir.join("eslint-cli"), b"exe").unwrap();

    jolter
        .storage()
        .activate_plugin_tool("@scope/eslint", "eslint-cli", &v1)
        .unwrap();

    let install_sub = PluginCommand::Install {
        target: "eslint@1.x".parse().unwrap(),
    };
    let _ = run_plugin(&jolter, install_sub, &ui);

    let list_sub = PluginCommand::List { json: false };
    let _ = run_plugin(&jolter, list_sub, &ui);

    let list_json_sub = PluginCommand::List { json: true };
    let _ = run_plugin(&jolter, list_json_sub, &ui);

    let _ = commands::inventory::print_inventory(&jolter, &ui);
    let _ = commands::inventory::print_inventory_json(&jolter);
    let _ = commands::inventory::print_plugins(&jolter, &ui);
    let _ = commands::inventory::print_plugins_json(&jolter);

    let update_sub = PluginCommand::Update {
        target: Some("eslint".to_owned()),
        all: false,
    };
    let _ = run_plugin(&jolter, update_sub, &ui);

    let uninstall_sub = PluginCommand::Uninstall {
        name: "eslint".to_owned(),
        force: true,
    };
    let _ = run_plugin(&jolter, uninstall_sub, &ui);
}

#[test]
fn tests_cli_args_parsing_and_helpers() {
    use crate::args::*;
    use clap::Parser;

    let cli_ls_json = Cli::parse_from(["jolter", "list", "--json"]);
    assert!(cli_ls_json.machine_output());

    let cli_doc_json = Cli::parse_from(["jolter", "doctor", "--json"]);
    assert!(cli_doc_json.machine_output());

    let cli_comp = Cli::parse_from(["jolter", "completions", "bash"]);
    assert!(cli_comp.machine_output());

    let cli_use = Cli::parse_from(["jolter", "use", "node@24"]);
    assert!(!cli_use.machine_output());

    assert!(parse_use_target("!!!").is_err());

    assert!(parse_use_target("custom-plugin-tool@1.0.0").is_ok());
    assert!(parse_use_target("custom-plugin-tool").is_ok());

    assert!(parse_update_target("!!!").is_err());

    assert!(parse_update_target("custom-plugin-tool@1.0.0").is_ok());
    assert!(parse_update_target("node@invalid").is_err());

    assert!(parse_uninstall_target("target_without_version").is_err());
    assert!(parse_uninstall_target("node@invalid-ver").is_err());
    assert!(parse_uninstall_target("custom-tool@1.0.0").is_ok());

    let pt_un = UninstallTarget::PluginTool {
        name: "custom".to_owned(),
        version: semver::Version::new(1, 0, 0),
    };
    assert_eq!(pt_un.to_string(), "custom@1.0.0");

    let pt_use = UseTarget::PluginTool {
        name: "custom".to_owned(),
        selector: "1.x".to_owned(),
    };
    assert_eq!(pt_use.to_string(), "custom@1.x");

    assert_eq!(
        jolter_core::ReleaseChannel::from(ChannelArg::Nightly),
        jolter_core::ReleaseChannel::Nightly
    );
    assert_eq!(
        jolter_core::ReleaseChannel::from(ChannelArg::Stable),
        jolter_core::ReleaseChannel::Stable
    );

    assert!(parse_use_target("@@@invalid!!!").is_err());
    assert!(parse_update_target("@@@invalid!!!").is_err());
    assert!(parse_uninstall_target("@@@invalid!!!@1.0.0").is_err());
    assert!(parse_plugin_request("invalid name with spaces").is_err());
}

#[test]
fn tests_setup_ci_and_github_actions_env() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = jolter_storage::Storage::new(temp_dir.path());
    storage.ensure_layout().unwrap();
    let jolter = Jolter::with_storage(storage).unwrap();
    let ui = Arc::new(TerminalUi::new(OutputOptions {
        progress: ProgressPreference::Plain,
        color: ColorPreference::Never,
        detail: DetailLevel::Normal,
        kind: OutputKind::Human,
    }));

    fs::write(
        temp_dir.path().join("jolter.json"),
        r#"{"runtime":{"node":"24.1.0"}}"#,
    )
    .unwrap();

    let node_ver = semver::Version::new(24, 1, 0);
    let node_exe = jolter
        .storage()
        .runtime_executable(RuntimeKind::Node, &node_ver);
    fs::create_dir_all(node_exe.parent().unwrap()).unwrap();
    fs::write(&node_exe, b"node").unwrap();

    let _ = run_setup_ci(&jolter, temp_dir.path(), false, true, &ui);
    let _ = run_setup_ci(&jolter, temp_dir.path(), true, true, &ui);
}

#[test]
fn tests_upgrade_cmd_and_additional_errors() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = jolter_storage::Storage::new(temp_dir.path());
    storage.ensure_layout().unwrap();
    let jolter = Jolter::with_storage(storage).unwrap();
    let ui = Arc::new(TerminalUi::new(OutputOptions {
        progress: ProgressPreference::Plain,
        color: ColorPreference::Never,
        detail: DetailLevel::Normal,
        kind: OutputKind::Human,
    }));

    let _ = run_upgrade(&jolter, jolter_core::ReleaseChannel::Stable, false, &ui);

    let outcome_dry = jolter_core::PruneOutcome {
        removed: vec![jolter_core::PruneItem {
            kind: jolter_core::PruneItemKind::Runtime(jolter_runtime::RuntimeKind::Node),
            version: semver::Version::new(22, 0, 0),
            path: std::path::PathBuf::from("/tmp/node"),
            reclaimed_bytes: 1024,
        }],
        dry_run: true,
    };
    commands::prune_cmd::print_prune_outcome(&outcome_dry, &ui);

    let outcome_applied = jolter_core::PruneOutcome {
        removed: vec![jolter_core::PruneItem {
            kind: jolter_core::PruneItemKind::Runtime(jolter_runtime::RuntimeKind::Node),
            version: semver::Version::new(22, 0, 0),
            path: std::path::PathBuf::from("/tmp/node"),
            reclaimed_bytes: 1024,
        }],
        dry_run: false,
    };
    commands::prune_cmd::print_prune_outcome(&outcome_applied, &ui);

    let cli_list_json = Cli {
        no_progress: false,
        no_color: false,
        quiet: false,
        verbose: false,
        command: Command::List { json: true },
    };
    assert!(cli_list_json.machine_output());

    let cli_doc_json = Cli {
        no_progress: false,
        no_color: false,
        quiet: false,
        verbose: false,
        command: Command::Doctor { json: true },
    };
    assert!(cli_doc_json.machine_output());

    let cli_setup_ci = Cli {
        no_progress: false,
        no_color: false,
        quiet: false,
        verbose: false,
        command: Command::SetupCi {
            json: true,
            yes: true,
        },
    };
    assert!(cli_setup_ci.machine_output());

    let cli_plugin_list_json = Cli {
        no_progress: false,
        no_color: false,
        quiet: false,
        verbose: false,
        command: Command::Plugin {
            command: PluginCommand::List { json: true },
        },
    };
    assert!(cli_plugin_list_json.machine_output());

    let cli_comp = Cli {
        no_progress: false,
        no_color: false,
        quiet: false,
        verbose: false,
        command: Command::Completions {
            shell: CompletionShell::Bash,
        },
    };
    assert!(cli_comp.machine_output());

    let cli_normal = Cli {
        no_progress: false,
        no_color: false,
        quiet: false,
        verbose: false,
        command: Command::List { json: false },
    };
    assert!(!cli_normal.machine_output());

    let err = CliError::InteractivePrompt("prompt failed".to_owned());
    assert!(err.to_string().contains("prompt failed"));

    let err = CliError::InteractiveSelection("invalid selection".to_owned());
    assert!(err.to_string().contains("invalid selection"));

    assert!(parse_use_target("!").is_err());
    assert!(parse_update_target("!").is_err());
    assert!(parse_uninstall_target("node").is_err());
    assert!(parse_uninstall_target("node@invalid").is_err());
    assert!(parse_uninstall_target("!@1.0.0").is_err());

    let pt_use = UseTarget::PluginTool {
        name: "my-tool".to_owned(),
        selector: "1.0".to_owned(),
    };
    assert_eq!(pt_use.to_string(), "my-tool@1.0");

    let pt_un = UninstallTarget::PluginTool {
        name: "my-tool".to_owned(),
        version: Version::new(1, 0, 0),
    };
    assert_eq!(pt_un.to_string(), "my-tool@1.0.0");

    let chan_st: jolter_core::ReleaseChannel = ChannelArg::Stable.into();
    assert_eq!(chan_st, jolter_core::ReleaseChannel::Stable);
    let chan_ng: jolter_core::ReleaseChannel = ChannelArg::Nightly.into();
    assert_eq!(chan_ng, jolter_core::ReleaseChannel::Nightly);
}
