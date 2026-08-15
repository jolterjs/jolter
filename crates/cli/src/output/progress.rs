use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

use jolter_core::{ProgressAction, ProgressEvent};

use super::style::format_duration;

pub(crate) const REDRAW_INTERVAL: Duration = Duration::from_millis(80);
pub(crate) const SPINNER: [&str; 4] = ["-", "\\", "|", "/"];
pub(crate) const TIMING_STEPS: [TimingStep; 8] = [
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
pub(crate) enum RenderMode {
    Interactive,
    Plain,
    Quiet,
    Silent,
}

#[derive(Default)]
pub(crate) struct ProgressState {
    pub(crate) visible_width: usize,
    pub(crate) frame: usize,
    pub(crate) last_draw: Option<Instant>,
    pub(crate) last_plain: Option<String>,
    pub(crate) download: Option<DownloadState>,
    pub(crate) timing: TimingRecorder,
}

pub(crate) struct DownloadState {
    pub(crate) name: String,
    pub(crate) total: Option<u64>,
    pub(crate) downloaded: u64,
    pub(crate) started: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TimingStep {
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
    pub(crate) const fn label(self) -> &'static str {
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

    pub(crate) const fn index(self) -> usize {
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
pub(crate) struct ActiveTimingStep {
    pub(crate) step: TimingStep,
    pub(crate) started: Instant,
}

#[derive(Debug, Default)]
pub(crate) struct TimingRecorder {
    pub(crate) totals: [Duration; TIMING_STEPS.len()],
    pub(crate) active: Option<ActiveTimingStep>,
}

impl TimingRecorder {
    pub(crate) fn observe(&mut self, step: TimingStep, now: Instant) {
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

    pub(crate) fn finish_current(&mut self, now: Instant) {
        let Some(active) = self.active.take() else {
            return;
        };
        let elapsed = now.saturating_duration_since(active.started);
        let index = active.step.index();
        self.totals[index] = self.totals[index].saturating_add(elapsed);
    }

    pub(crate) fn total_for(&self, step: TimingStep) -> Duration {
        self.totals[step.index()]
    }
}

pub(crate) fn timing_step_for_event(event: &ProgressEvent<'_>) -> TimingStep {
    match event {
        ProgressEvent::Stage { action, .. } => timing_step_for_action(*action),
        ProgressEvent::DownloadStarted { .. }
        | ProgressEvent::DownloadAdvanced { .. }
        | ProgressEvent::DownloadFinished { .. } => TimingStep::Download,
        ProgressEvent::CacheHit { .. } => TimingStep::Resolve,
    }
}

pub(crate) const fn timing_step_for_action(action: ProgressAction) -> TimingStep {
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

pub(crate) fn format_timing_line(timing: &TimingRecorder, total: Duration) -> String {
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

pub(crate) fn render_dynamic_line(state: &mut ProgressState, plain: &str, styled: &str) {
    let width = plain.chars().count();
    let padding = state.visible_width.saturating_sub(width);
    let mut stderr = io::stderr().lock();
    let _ = write!(stderr, "\r{styled}{}", " ".repeat(padding));
    let _ = stderr.flush();
    state.visible_width = width;
}

pub(crate) fn clear_dynamic_line(state: &mut ProgressState) {
    let mut stderr = io::stderr().lock();
    let _ = write!(stderr, "\r{}\r", " ".repeat(state.visible_width));
    let _ = stderr.flush();
    state.visible_width = 0;
}

pub(crate) fn progress_bar(downloaded: u64, total: u64, width: usize) -> String {
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
