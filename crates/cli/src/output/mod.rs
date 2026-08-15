use std::{
    env,
    io::{self, IsTerminal},
    sync::Mutex,
    time::{Duration, Instant},
};

use jolter_core::{ProgressAction, ProgressEvent, ProgressReporter};

pub mod options;
pub mod progress;
pub mod style;
pub mod table;

#[cfg(test)]
mod tests;

pub use options::{ColorPreference, DetailLevel, OutputKind, OutputOptions, ProgressPreference};
use progress::{
    DownloadState, ProgressState, REDRAW_INTERVAL, RenderMode, SPINNER, clear_dynamic_line,
    format_timing_line, progress_bar, render_dynamic_line, timing_step_for_event,
};
use style::{
    Color, env_flag, paint, rate, terminal_supports_ansi, write_stderr_line, write_stdout_line,
};
pub use style::{format_bytes, format_duration};
pub use table::{TableRow, format_table_rows};

pub struct TerminalUi {
    stdout_color: bool,
    stderr_color: bool,
    detail: DetailLevel,
    mode: RenderMode,
    state: Mutex<ProgressState>,
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
