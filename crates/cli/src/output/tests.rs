use super::*;
use std::time::{Duration, Instant};

use progress::{TIMING_STEPS, TimingRecorder, TimingStep, timing_step_for_action};
use style::windows_ansi_advertised;

#[test]
fn renders_bounded_ascii_progress_bars() {
    assert_eq!(progress_bar(0, 100, 5), "[>....]");
    assert_eq!(progress_bar(50, 100, 5), "[==>..]");
    assert_eq!(progress_bar(100, 100, 5), "[=====]");
    assert_eq!(progress_bar(1, 0, 5), "[]");
}

#[test]
fn formats_transfer_values_and_rates() {
    assert_eq!(format_bytes(512), "512 B");
    assert_eq!(format_bytes(1536), "1.5 KiB");
    assert_eq!(rate(2048, Duration::from_secs(2)), 1024);
    assert_eq!(format_duration(Duration::from_millis(250)), "250ms");
    assert_eq!(format_duration(Duration::from_millis(1250)), "1s 250ms");
    assert_eq!(
        format_duration(Duration::from_millis(
            86_400_000 + 7_200_000 + 180_000 + 4_000 + 5
        )),
        "1d 2h 3m 4s 5ms"
    );
}

#[test]
fn aggregates_timing_steps_into_one_line() {
    let start = Instant::now();
    let mut timing = TimingRecorder::default();
    timing.observe(TimingStep::Resolve, start);
    timing.observe(TimingStep::Download, start + Duration::from_millis(8));
    timing.observe(TimingStep::Install, start + Duration::from_millis(20));
    timing.finish_current(start + Duration::from_millis(45));

    assert_eq!(
        format_timing_line(&timing, Duration::from_millis(50)),
        "resolve 8ms | download 12ms | install 25ms | total 50ms"
    );
}

#[test]
fn aligns_table_status_and_detail_columns() {
    let rows = [
        TableRow::new(' ', "bun@1.2.3", "[incomplete]", "C:\\runtimes\\bun"),
        TableRow::new('*', "node@24.16.0", "[ready]", "C:\\runtimes\\node"),
    ];

    let lines = format_table_rows(&rows);

    assert_eq!(lines[0].find("[incomplete]"), lines[1].find("[ready]"));
    assert_eq!(
        lines[0].find("C:\\runtimes\\bun"),
        lines[1].find("C:\\runtimes\\node")
    );
    assert!(lines.iter().all(|line| !line.contains('\t')));
}

#[test]
fn windows_ansi_requires_an_advertised_capable_host() {
    assert!(!windows_ansi_advertised(
        false, false, None, None, None, None
    ));
    assert!(windows_ansi_advertised(true, false, None, None, None, None));
    assert!(windows_ansi_advertised(
        false,
        false,
        None,
        Some("xterm-256color"),
        None,
        None
    ));
    assert!(!windows_ansi_advertised(
        false,
        false,
        None,
        Some("dumb"),
        None,
        None
    ));
}

#[test]
fn tests_terminal_ui_creation_and_reporting() {
    let opts = OutputOptions {
        progress: ProgressPreference::Plain,
        color: ColorPreference::Never,
        detail: DetailLevel::Normal,
        kind: OutputKind::Human,
    };
    let ui = TerminalUi::new(opts);
    ui.success("operation successful");
    ui.info("info message");
    ui.warning("warning message");
    ui.failure("failure message");
    ui.heading("Heading Title");
    ui.line("regular line");
    ui.detail("detail note");

    let rows = [TableRow::new('-', "item", "ok", "path")];
    ui.table(&rows);
    ui.timing(Duration::from_millis(150));

    ui.report(ProgressEvent::Stage {
        action: ProgressAction::Select,
        target: "node@20",
    });
    ui.report(ProgressEvent::DownloadStarted {
        name: "node.tar.gz",
        total: Some(1024),
    });
    ui.report(ProgressEvent::DownloadAdvanced {
        name: "node.tar.gz",
        downloaded: 512,
        total: Some(1024),
    });
    ui.report(ProgressEvent::DownloadFinished {
        name: "node.tar.gz",
        downloaded: 1024,
        total: Some(1024),
    });
    ui.report(ProgressEvent::CacheHit { name: "node@20" });

    ui.finish_progress();
}

#[test]
fn tests_terminal_ui_quiet_and_machine_modes() {
    let opts = OutputOptions {
        progress: ProgressPreference::Auto,
        color: ColorPreference::Never,
        detail: DetailLevel::Quiet,
        kind: OutputKind::Machine,
    };
    let ui = TerminalUi::new(opts);
    ui.detail("should be ignored in quiet mode");
    ui.timing(Duration::from_millis(50));
    ui.report(ProgressEvent::Stage {
        action: ProgressAction::Activate,
        target: "node",
    });
}

#[test]
fn tests_paint_and_color_codes() {
    assert_eq!(paint(false, Color::Red, "test"), "test");
    assert_eq!(paint(true, Color::Red, "test"), "\x1b[31mtest\x1b[0m");
    assert_eq!(paint(true, Color::Green, "test"), "\x1b[32mtest\x1b[0m");
    assert_eq!(paint(true, Color::Yellow, "test"), "\x1b[33mtest\x1b[0m");
    assert_eq!(paint(true, Color::Cyan, "test"), "\x1b[36mtest\x1b[0m");
    assert_eq!(paint(true, Color::Dim, "test"), "\x1b[2mtest\x1b[0m");
    assert_eq!(paint(true, Color::Gray, "test"), "\x1b[90mtest\x1b[0m");
}

#[test]
fn tests_format_bytes_large_units() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(1024 * 1024), "1.0 MiB");
    assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GiB");
    assert_eq!(format_bytes(1024 * 1024 * 1024 * 1024), "1.0 TiB");
}

#[test]
fn tests_verbose_ui_mode() {
    let opts = OutputOptions {
        progress: ProgressPreference::Plain,
        color: ColorPreference::Never,
        detail: DetailLevel::Verbose,
        kind: OutputKind::Human,
    };
    let ui = TerminalUi::new(opts);
    ui.success("verbose op");
    ui.detail("verbose detail");
    ui.timing(Duration::from_millis(200));

    ui.report(ProgressEvent::DownloadStarted {
        name: "pkg.tar.gz",
        total: Some(2048),
    });
    ui.report(ProgressEvent::DownloadAdvanced {
        name: "pkg.tar.gz",
        downloaded: 1024,
        total: Some(2048),
    });
    ui.report(ProgressEvent::DownloadFinished {
        name: "pkg.tar.gz",
        downloaded: 2048,
        total: Some(2048),
    });
}

#[test]
fn tests_machine_mode_suppresses_all_output() {
    let opts = OutputOptions {
        progress: ProgressPreference::Auto,
        color: ColorPreference::Never,
        detail: DetailLevel::Normal,
        kind: OutputKind::Machine,
    };
    let ui = TerminalUi::new(opts);
    ui.success("should not appear");
    ui.info("machine info");
    ui.warning("machine warn");
    ui.failure("machine fail");
    ui.report(ProgressEvent::Stage {
        action: ProgressAction::Select,
        target: "node@20",
    });
    ui.report(ProgressEvent::DownloadStarted {
        name: "test.tar.gz",
        total: None,
    });
    ui.report(ProgressEvent::CacheHit { name: "cached" });
    ui.timing(Duration::from_millis(10));
    ui.finish_progress();
}

#[test]
fn tests_rate_and_duration_edge_cases() {
    assert_eq!(rate(1000, Duration::ZERO), 0);
    assert_eq!(format_duration(Duration::ZERO), "0ms");
    assert_eq!(format_duration(Duration::from_secs(60)), "1m");
    assert_eq!(format_duration(Duration::from_secs(3600)), "1h");

    assert_eq!(progress_bar(0, 0, 5), "[]");
    assert_eq!(progress_bar(0, 100, 0), "[]");
    assert_eq!(progress_bar(200, 100, 5), "[=====]");
}

#[test]
fn tests_env_flag_returns_false_for_unset() {
    assert!(!env_flag("JOLTER_TEST_UNSET_VAR_XYZ_12345"));
}

#[test]
fn tests_timing_step_labels_and_indices() {
    for (i, step) in TIMING_STEPS.iter().enumerate() {
        assert!(!step.label().is_empty());
        assert_eq!(step.index(), i);
    }
}

#[test]
fn tests_timing_recorder_observe_deduplicates() {
    let start = Instant::now();
    let mut timing = TimingRecorder::default();
    timing.observe(TimingStep::Resolve, start);
    timing.observe(TimingStep::Resolve, start + Duration::from_millis(5));
    timing.finish_current(start + Duration::from_millis(10));
    assert!(timing.total_for(TimingStep::Resolve) >= Duration::from_millis(9));
}

#[test]
fn tests_timing_step_for_all_actions() {
    let actions = [
        ProgressAction::Select,
        ProgressAction::Resolve,
        ProgressAction::Reuse,
        ProgressAction::Connect,
        ProgressAction::Download,
        ProgressAction::Verify,
        ProgressAction::Extract,
        ProgressAction::Publish,
        ProgressAction::Activate,
        ProgressAction::Shims,
        ProgressAction::Remove,
        ProgressAction::Clean,
        ProgressAction::Diagnose,
        ProgressAction::Configure,
    ];
    for action in actions {
        let _ = timing_step_for_action(action);
    }
}

#[test]
fn tests_table_row_construction() {
    let row = TableRow::new('*', "node@24.1.0", "active", "/path/to/node");
    assert_eq!(row.marker, '*');
    assert_eq!(row.label, "node@24.1.0");
    assert_eq!(row.status, "active");
    assert_eq!(row.detail, "/path/to/node");
}

#[test]
fn tests_empty_table_rows() {
    let rows: &[TableRow] = &[];
    let lines = format_table_rows(rows);
    assert!(lines.is_empty());
}

#[test]
fn tests_color_preference_auto() {
    let opts = OutputOptions {
        progress: ProgressPreference::Auto,
        color: ColorPreference::Auto,
        detail: DetailLevel::Normal,
        kind: OutputKind::Human,
    };
    let ui = TerminalUi::new(opts);
    ui.success("colored success");
    ui.failure("colored failure");
}
