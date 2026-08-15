use std::{
    env,
    io::{self, Write},
    time::Duration,
};

#[derive(Clone, Copy)]
pub(crate) enum Color {
    Red,
    Green,
    Yellow,
    Cyan,
    Dim,
    Gray,
}

impl Color {
    pub(crate) const fn code(self) -> &'static str {
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

pub(crate) fn paint(enabled: bool, color: Color, text: &str) -> String {
    if enabled {
        format!("\x1b[{}m{text}\x1b[0m", color.code())
    } else {
        text.to_owned()
    }
}

pub(crate) fn terminal_supports_ansi(is_terminal: bool) -> bool {
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
pub(crate) fn windows_ansi_advertised(
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

pub(crate) fn write_stdout_line(line: &str) {
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
    let _ = stdout.flush();
}

pub(crate) fn write_stderr_line(line: &str) {
    let mut stderr = io::stderr().lock();
    let _ = writeln!(stderr, "{line}");
    let _ = stderr.flush();
}

pub(crate) fn rate(bytes: u64, elapsed: Duration) -> u64 {
    let millis = elapsed.as_millis();
    if millis == 0 {
        return 0;
    }
    u64::try_from(u128::from(bytes).saturating_mul(1000) / millis).unwrap_or(u64::MAX)
}

pub fn format_duration(duration: Duration) -> String {
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

pub fn format_bytes(bytes: u64) -> String {
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

pub(crate) fn env_flag(name: &str) -> bool {
    env::var_os(name).is_some_and(|value| {
        matches!(
            value.to_string_lossy().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        )
    })
}
