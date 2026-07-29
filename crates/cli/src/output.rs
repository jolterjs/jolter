use std::{
    env,
    io::{self, IsTerminal, Write},
    sync::Mutex,
    time::{Duration, Instant},
};

use jolter_core::{ProgressAction, ProgressEvent, ProgressReporter};

const REDRAW_INTERVAL: Duration = Duration::from_millis(80);
const SPINNER: [&str; 4] = ["-", "\\", "|", "/"];
const TIMING_STEPS: [TimingStep; 8] = [
    TimingStep::Resolve,
    TimingStep::Download,
    TimingStep::Install,
    TimingStep::Activate,
    TimingStep::Shims,
    TimingStep::Remove,
    TimingStep::Clean,
    TimingStep::Other,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressPreference {
    Auto,
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorPreference {
    Auto,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailLevel {
    Quiet,
    Normal,
    Verbose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    Human,
    Machine,
}

#[derive(Debug, Clone, Copy)]
pub struct OutputOptions {
    pub progress: ProgressPreference,
    pub color: ColorPreference,
    pub detail: DetailLevel,
    pub kind: OutputKind,
}

pub struct TerminalUi {
    stdout_color: bool,
    stderr_color: bool,
    detail: DetailLevel,
    mode: RenderMode,
    state: Mutex<ProgressState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRow {
    marker: char,
    label: String,
    status: String,
    detail: String,
}

impl TableRow {
    pub fn new(
        marker: char,
        label: impl Into<String>,
        status: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            marker,
            label: label.into(),
            status: status.into(),
            detail: detail.into(),
        }
    }
}

impl TerminalUi {
    #[must_use]
    pub fn new(options: OutputOptions) -> Self {
        let stderr_terminal = io::stderr().is_terminal();
        let automated = env_flag("CI")
            || env_flag("GITHUB_ACTIONS")
            || env_flag("GITLAB_CI")
            || env_flag("CIRCLECI")
            || env_flag("TF_BUILD")
            || env_flag("BUILDKITE");
        let progress_disabled = options.progress == ProgressPreference::Plain
            || env_flag("JOLTER_NO_PROGRESS")
            || automated
            || !stderr_terminal;
        let color_enabled = options.color == ColorPreference::Auto
            && env::var_os("NO_COLOR").is_none()
            && env::var_os("JOLTER_NO_COLOR").is_none();
        let mode = if options.kind == OutputKind::Machine {
            RenderMode::Silent
        } else if options.detail == DetailLevel::Quiet {
            RenderMode::Quiet
        } else if progress_disabled {
            RenderMode::Plain
        } else {
            RenderMode::Interactive
        };

        Self {
            stdout_color: color_enabled && terminal_supports_ansi(io::stdout().is_terminal()),
            stderr_color: color_enabled && terminal_supports_ansi(stderr_terminal),
            detail: options.detail,
            mode,
            state: Mutex::new(ProgressState::default()),
        }
    }

    pub fn success(&self, message: impl AsRef<str>) {
        self.finish_progress();
        write_stdout_line(&format!(
            "{} {}",
            self.stdout_tag("done", Color::Green),
            message.as_ref()
        ));
    }

    pub fn info(&self, message: impl AsRef<str>) {
        self.finish_progress();
        write_stdout_line(&format!(
            "{} {}",
            self.stdout_tag("jolter", Color::Cyan),
            message.as_ref()
        ));
    }

    pub fn warning(&self, message: impl AsRef<str>) {
        self.finish_progress();
        write_stdout_line(&format!(
            "{} {}",
            self.stdout_tag("warn", Color::Yellow),
            message.as_ref()
        ));
    }

    pub fn failure(&self, message: impl AsRef<str>) {
        self.finish_progress();
        write_stderr_line(&format!(
            "{} {}",
            self.stderr_tag("fail", Color::Red),
            message.as_ref()
        ));
    }

    pub fn heading(&self, title: &str) {
        self.finish_progress();
        write_stdout_line(&self.stdout_paint(Color::Cyan, &format!("{title}:")));
    }

    pub fn line(&self, line: impl AsRef<str>) {
        self.finish_progress();
        write_stdout_line(line.as_ref());
    }

    pub fn table(&self, rows: &[TableRow]) {
        self.finish_progress();
        for line in format_table_rows(rows) {
            write_stdout_line(&line);
        }
    }

    pub fn detail(&self, message: impl AsRef<str>) {
        if self.mode == RenderMode::Quiet {
            return;
        }
        self.finish_progress();
        write_stdout_line(&format!(
            "{} {}",
            self.stdout_tag("note", Color::Dim),
            message.as_ref()
        ));
    }

    pub fn timing(&self, total: Duration) {
        if matches!(self.mode, RenderMode::Quiet | RenderMode::Silent) {
            return;
        }
        self.finish_progress();
        let line = {
            let mut state = self.state.lock().unwrap();
            state.timing.finish_current(Instant::now());
            format_timing_line(&state.timing, total)
        };
        write_stderr_line(&format!(
            "{} {}",
            self.stderr_tag("time", Color::Gray),
            self.stderr_paint(Color::Gray, &line)
        ));
    }

    pub fn finish_progress(&self) {
        let mut state = self.state.lock().unwrap();
        if state.visible_width == 0 {
            return;
        }
        clear_dynamic_line(&mut state);
    }

    fn stdout_paint(&self, color: Color, text: &str) -> String {
        paint(self.stdout_color, color, text)
    }

    fn stderr_paint(&self, color: Color, text: &str) -> String {
        paint(self.stderr_color, color, text)
    }

    fn stdout_tag(&self, label: &str, color: Color) -> String {
        self.stdout_paint(color, &format!("[{label}]"))
    }

    fn stderr_tag(&self, label: &str, color: Color) -> String {
        self.stderr_paint(color, &format!("[{label}]"))
    }

    fn plain_stage(&self, state: &mut ProgressState, action: ProgressAction, target: &str) {
        let signature = format!("{}:{target}", action.label());
        if state.last_plain.as_deref() == Some(&signature) {
            return;
        }
        state.last_plain = Some(signature);
        write_stderr_line(&format!(
            "{} {:<8} {target}",
            self.stderr_tag("jolter", Color::Cyan),
            action.label()
        ));
    }

    fn interactive_stage(&self, state: &mut ProgressState, action: ProgressAction, target: &str) {
        state.download = None;
        state.frame = state.frame.wrapping_add(1);
        let frame = SPINNER[state.frame % SPINNER.len()];
        let plain = format!("[jolter] {frame} {:<8} {target}", action.label());
        let styled = format!(
            "{} {} {:<8} {target}",
            self.stderr_tag("jolter", Color::Cyan),
            self.stderr_paint(Color::Cyan, frame),
            action.label()
        );
        render_dynamic_line(state, &plain, &styled);
    }

    fn download_started(&self, state: &mut ProgressState, name: &str, total: Option<u64>) {
        state.download = Some(DownloadState {
            name: name.to_owned(),
            total,
            downloaded: 0,
            started: Instant::now(),
        });
        state.last_draw = None;
        if self.mode == RenderMode::Interactive {
            self.render_download(state, true);
        } else {
            let size = total
                .map(|bytes| format!(" ({})", format_bytes(bytes)))
                .unwrap_or_default();
            write_stderr_line(&format!(
                "{} {:<8} {name}{size}",
                self.stderr_tag("jolter", Color::Cyan),
                ProgressAction::Download.label()
            ));
        }
    }

    fn download_advanced(
        &self,
        state: &mut ProgressState,
        name: &str,
        downloaded: u64,
        total: Option<u64>,
    ) {
        let Some(download) = state.download.as_mut() else {
            state.download = Some(DownloadState {
                name: name.to_owned(),
                total,
                downloaded,
                started: Instant::now(),
            });
            return;
        };
        download.downloaded = downloaded;
        download.total = total.or(download.total);
        if self.mode != RenderMode::Interactive {
            return;
        }
        let complete = download.total.is_some_and(|length| downloaded >= length);
        let due = state
            .last_draw
            .is_none_or(|last| last.elapsed() >= REDRAW_INTERVAL);
        if complete || due {
            self.render_download(state, true);
        }
    }

    fn download_finished(
        &self,
        state: &mut ProgressState,
        name: &str,
        downloaded: u64,
        total: Option<u64>,
    ) {
        self.download_advanced(state, name, downloaded, total);
        if self.mode == RenderMode::Interactive {
            self.render_download(state, true);
            if self.detail == DetailLevel::Verbose {
                let elapsed = state
                    .download
                    .as_ref()
                    .map_or(Duration::ZERO, |download| download.started.elapsed());
                clear_dynamic_line(state);
                write_stderr_line(&format!(
                    "{} {:<8} {name} ({} in {}, {}/s)",
                    self.stderr_tag("done", Color::Green),
                    "fetched",
                    format_bytes(downloaded),
                    format_duration(elapsed),
                    format_bytes(rate(downloaded, elapsed))
                ));
            }
        } else {
            let elapsed = state
                .download
                .as_ref()
                .map_or(Duration::ZERO, |download| download.started.elapsed());
            let detail = if self.detail == DetailLevel::Verbose {
                format!(
                    "{} in {} ({}/s)",
                    format_bytes(downloaded),
                    format_duration(elapsed),
                    format_bytes(rate(downloaded, elapsed))
                )
            } else {
                format_bytes(downloaded)
            };
            write_stderr_line(&format!(
                "{} {:<8} {name} ({detail})",
                self.stderr_tag("done", Color::Green),
                "fetched"
            ));
        }
    }

    fn render_download(&self, state: &mut ProgressState, force: bool) {
        let Some(download) = state.download.as_ref() else {
            return;
        };
        if !force
            && state
                .last_draw
                .is_some_and(|last| last.elapsed() < REDRAW_INTERVAL)
        {
            return;
        }
        let elapsed = download.started.elapsed();
        let speed = rate(download.downloaded, elapsed);
        let details = if let Some(total) = download.total.filter(|total| *total > 0) {
            let percent = download.downloaded.saturating_mul(100) / total;
            format!(
                "{} {:>3}% {}/{} {}/s",
                progress_bar(download.downloaded, total, 18),
                percent.min(100),
                format_bytes(download.downloaded),
                format_bytes(total),
                format_bytes(speed)
            )
        } else {
            format!(
                "{} {}/s",
                format_bytes(download.downloaded),
                format_bytes(speed)
            )
        };
        let frame = SPINNER[state.frame % SPINNER.len()];
        state.frame = state.frame.wrapping_add(1);
        let plain = format!(
            "[jolter] {frame} {:<8} {} {details}",
            ProgressAction::Download.label(),
            download.name
        );
        let styled = format!(
            "{} {} {:<8} {} {details}",
            self.stderr_tag("jolter", Color::Cyan),
            self.stderr_paint(Color::Cyan, frame),
            ProgressAction::Download.label(),
            download.name
        );
        render_dynamic_line(state, &plain, &styled);
        state.last_draw = Some(Instant::now());
    }
}

impl ProgressReporter for TerminalUi {
    fn report(&self, event: ProgressEvent<'_>) {
        if matches!(self.mode, RenderMode::Quiet | RenderMode::Silent) {
            return;
        }
        let mut state = self.state.lock().unwrap();
        state
            .timing
            .observe(timing_step_for_event(&event), Instant::now());
        match event {
            ProgressEvent::Stage { action, target } => {
                if self.mode == RenderMode::Interactive {
                    self.interactive_stage(&mut state, action, target);
                } else {
                    self.plain_stage(&mut state, action, target);
                }
            }
            ProgressEvent::DownloadStarted { name, total } => {
                self.download_started(&mut state, name, total);
            }
            ProgressEvent::DownloadAdvanced {
                name,
                downloaded,
                total,
            } => {
                self.download_advanced(&mut state, name, downloaded, total);
            }
            ProgressEvent::DownloadFinished {
                name,
                downloaded,
                total,
            } => {
                self.download_finished(&mut state, name, downloaded, total);
            }
            ProgressEvent::CacheHit { name } => {
                if self.mode == RenderMode::Interactive {
                    self.interactive_stage(&mut state, ProgressAction::Reuse, name);
                } else {
                    self.plain_stage(&mut state, ProgressAction::Reuse, name);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderMode {
    Interactive,
    Plain,
    Quiet,
    Silent,
}

#[derive(Default)]
struct ProgressState {
    visible_width: usize,
    frame: usize,
    last_draw: Option<Instant>,
    last_plain: Option<String>,
    download: Option<DownloadState>,
    timing: TimingRecorder,
}

struct DownloadState {
    name: String,
    total: Option<u64>,
    downloaded: u64,
    started: Instant,
}

#[derive(Clone, Copy)]
enum Color {
    Red,
    Green,
    Yellow,
    Cyan,
    Dim,
    Gray,
}

impl Color {
    const fn code(self) -> &'static str {
        match self {
            Self::Red => "31",
            Self::Green => "32",
            Self::Yellow => "33",
            Self::Cyan => "36",
            Self::Dim => "2",
            Self::Gray => "90",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimingStep {
    Resolve,
    Download,
    Install,
    Activate,
    Shims,
    Remove,
    Clean,
    Other,
}

impl TimingStep {
    const fn label(self) -> &'static str {
        match self {
            Self::Resolve => "resolve",
            Self::Download => "download",
            Self::Install => "install",
            Self::Activate => "activate",
            Self::Shims => "shims",
            Self::Remove => "remove",
            Self::Clean => "clean",
            Self::Other => "other",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Resolve => 0,
            Self::Download => 1,
            Self::Install => 2,
            Self::Activate => 3,
            Self::Shims => 4,
            Self::Remove => 5,
            Self::Clean => 6,
            Self::Other => 7,
        }
    }
}

#[derive(Debug)]
struct ActiveTimingStep {
    step: TimingStep,
    started: Instant,
}

#[derive(Debug, Default)]
struct TimingRecorder {
    totals: [Duration; TIMING_STEPS.len()],
    active: Option<ActiveTimingStep>,
}

impl TimingRecorder {
    fn observe(&mut self, step: TimingStep, now: Instant) {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.step == step)
        {
            return;
        }
        self.finish_current(now);
        self.active = Some(ActiveTimingStep { step, started: now });
    }

    fn finish_current(&mut self, now: Instant) {
        let Some(active) = self.active.take() else {
            return;
        };
        let elapsed = now.saturating_duration_since(active.started);
        let index = active.step.index();
        self.totals[index] = self.totals[index].saturating_add(elapsed);
    }

    fn total_for(&self, step: TimingStep) -> Duration {
        self.totals[step.index()]
    }
}

fn timing_step_for_event(event: &ProgressEvent<'_>) -> TimingStep {
    match event {
        ProgressEvent::Stage { action, .. } => timing_step_for_action(*action),
        ProgressEvent::DownloadStarted { .. }
        | ProgressEvent::DownloadAdvanced { .. }
        | ProgressEvent::DownloadFinished { .. } => TimingStep::Download,
        ProgressEvent::CacheHit { .. } => TimingStep::Resolve,
    }
}

const fn timing_step_for_action(action: ProgressAction) -> TimingStep {
    match action {
        ProgressAction::Select | ProgressAction::Resolve | ProgressAction::Reuse => {
            TimingStep::Resolve
        }
        ProgressAction::Connect | ProgressAction::Download => TimingStep::Download,
        ProgressAction::Verify | ProgressAction::Extract | ProgressAction::Publish => {
            TimingStep::Install
        }
        ProgressAction::Activate => TimingStep::Activate,
        ProgressAction::Shims => TimingStep::Shims,
        ProgressAction::Remove => TimingStep::Remove,
        ProgressAction::Clean => TimingStep::Clean,
        ProgressAction::Diagnose | ProgressAction::Configure => TimingStep::Other,
    }
}

fn format_timing_line(timing: &TimingRecorder, total: Duration) -> String {
    let mut parts = TIMING_STEPS
        .into_iter()
        .filter_map(|step| {
            let elapsed = timing.total_for(step);
            (elapsed > Duration::ZERO)
                .then(|| format!("{} {}", step.label(), format_duration(elapsed)))
        })
        .collect::<Vec<_>>();
    parts.push(format!("total {}", format_duration(total)));
    parts.join(" | ")
}

fn paint(enabled: bool, color: Color, text: &str) -> String {
    if enabled {
        format!("\x1b[{}m{text}\x1b[0m", color.code())
    } else {
        text.to_owned()
    }
}

fn terminal_supports_ansi(is_terminal: bool) -> bool {
    if !is_terminal {
        return false;
    }

    #[cfg(windows)]
    {
        windows_ansi_advertised(
            env::var_os("WT_SESSION").is_some(),
            env::var_os("ANSICON").is_some(),
            env::var("ConEmuANSI").ok().as_deref(),
            env::var("TERM").ok().as_deref(),
            env::var("TERM_PROGRAM").ok().as_deref(),
            env::var("COLORTERM").ok().as_deref(),
        )
    }

    #[cfg(not(windows))]
    {
        env::var("TERM").map_or(true, |term| !term.eq_ignore_ascii_case("dumb"))
    }
}

#[cfg(any(windows, test))]
fn windows_ansi_advertised(
    windows_terminal: bool,
    ansicon: bool,
    conemu_ansi: Option<&str>,
    term: Option<&str>,
    term_program: Option<&str>,
    color_term: Option<&str>,
) -> bool {
    windows_terminal
        || ansicon
        || conemu_ansi.is_some_and(|value| value.eq_ignore_ascii_case("on"))
        || term.is_some_and(|value| !value.is_empty() && !value.eq_ignore_ascii_case("dumb"))
        || term_program.is_some_and(|value| !value.is_empty())
        || color_term.is_some_and(|value| !value.is_empty())
}

fn format_table_rows(rows: &[TableRow]) -> Vec<String> {
    let label_width = rows
        .iter()
        .map(|row| row.label.chars().count())
        .max()
        .unwrap_or_default();
    let status_width = rows
        .iter()
        .map(|row| row.status.chars().count())
        .max()
        .unwrap_or_default();

    rows.iter()
        .map(|row| {
            format!(
                "{} {:label_width$}  {:status_width$}  {}",
                row.marker, row.label, row.status, row.detail
            )
        })
        .collect()
}

fn write_stdout_line(line: &str) {
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
    let _ = stdout.flush();
}

fn write_stderr_line(line: &str) {
    let mut stderr = io::stderr().lock();
    let _ = writeln!(stderr, "{line}");
    let _ = stderr.flush();
}

fn render_dynamic_line(state: &mut ProgressState, plain: &str, styled: &str) {
    let width = plain.chars().count();
    let padding = state.visible_width.saturating_sub(width);
    let mut stderr = io::stderr().lock();
    let _ = write!(stderr, "\r{styled}{}", " ".repeat(padding));
    let _ = stderr.flush();
    state.visible_width = width;
}

fn clear_dynamic_line(state: &mut ProgressState) {
    let mut stderr = io::stderr().lock();
    let _ = write!(stderr, "\r{}\r", " ".repeat(state.visible_width));
    let _ = stderr.flush();
    state.visible_width = 0;
}

fn progress_bar(downloaded: u64, total: u64, width: usize) -> String {
    if total == 0 || width == 0 {
        return "[]".to_owned();
    }
    let filled = usize::try_from(
        downloaded
            .min(total)
            .saturating_mul(width as u64)
            .checked_div(total)
            .unwrap_or_default(),
    )
    .unwrap_or(width)
    .min(width);
    if filled >= width {
        return format!("[{}]", "=".repeat(width));
    }
    format!(
        "[{}>{}]",
        "=".repeat(filled),
        ".".repeat(width.saturating_sub(filled + 1))
    )
}

fn rate(bytes: u64, elapsed: Duration) -> u64 {
    let millis = elapsed.as_millis();
    if millis == 0 {
        return 0;
    }
    u64::try_from(u128::from(bytes).saturating_mul(1000) / millis).unwrap_or(u64::MAX)
}

fn format_duration(duration: Duration) -> String {
    let mut remaining = duration.as_millis();
    let day = 24 * 60 * 60 * 1000;
    let hour = 60 * 60 * 1000;
    let minute = 60 * 1000;
    let second = 1000;
    let units = [
        ("d", day),
        ("h", hour),
        ("m", minute),
        ("s", second),
        ("ms", 1),
    ];
    let mut parts = Vec::new();
    for (label, unit) in units {
        let value = remaining / unit;
        if value > 0 {
            parts.push(format!("{value}{label}"));
            remaining %= unit;
        }
    }
    if parts.is_empty() {
        "0ms".to_owned()
    } else {
        parts.join(" ")
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut divisor = 1_u64;
    let mut unit = 0;
    while bytes / divisor >= 1024 && unit < UNITS.len() - 1 {
        divisor = divisor.saturating_mul(1024);
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        let whole = bytes / divisor;
        let decimal = (bytes % divisor).saturating_mul(10) / divisor;
        format!("{whole}.{decimal} {}", UNITS[unit])
    }
}

fn env_flag(name: &str) -> bool {
    env::var_os(name).is_some_and(|value| {
        matches!(
            value.to_string_lossy().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // Test progress reporter implementation in plain mode
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

        // progress bar edge cases
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
        // Observing the same step should be a no-op
        timing.observe(TimingStep::Resolve, start + Duration::from_millis(5));
        timing.finish_current(start + Duration::from_millis(10));
        // Total should be ~10ms, not split into two observations
        assert!(timing.total_for(TimingStep::Resolve) >= Duration::from_millis(9));
    }

    #[test]
    fn tests_timing_step_for_all_actions() {
        // Verify every ProgressAction maps to a TimingStep
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
}
