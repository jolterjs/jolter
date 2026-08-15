use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    process::{Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use semver::Version;

use crate::{MAX_PROBE_OUTPUT, PROBE_TIMEOUT, types::Check};

#[must_use]
pub fn version_probe_check(
    name: &'static str,
    command: Command,
    expected: &Version,
    remediation: &'static str,
) -> Check {
    match run_probe(command) {
        Ok(output) if output.timed_out => Check::fail(
            name,
            format!("version probe exceeded {} seconds", PROBE_TIMEOUT.as_secs()),
            remediation,
        ),
        Ok(output) if !output.status.success() => Check::fail(
            name,
            format!(
                "version probe exited with {}: {}",
                output.status,
                output.combined_output()
            ),
            remediation,
        ),
        Ok(output) => match extract_version(&output.combined_output()) {
            Some(actual) if &actual == expected => {
                Check::pass(name, format!("reported version matches {expected}"))
            }
            Some(actual) => Check::fail(
                name,
                format!("reported version {actual} does not match installed version {expected}"),
                remediation,
            ),
            None => Check::fail(
                name,
                format!(
                    "could not parse a semantic version from `{}`",
                    output.combined_output()
                ),
                remediation,
            ),
        },
        Err(error) => Check::fail(name, format!("version probe failed: {error}"), remediation),
    }
}

pub struct ProbeOutput {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

impl ProbeOutput {
    #[must_use]
    pub fn combined_output(&self) -> String {
        let stdout = self.stdout.trim();
        let stderr = self.stderr.trim();
        match (stdout.is_empty(), stderr.is_empty()) {
            (false, true) => stdout.to_owned(),
            (true, false) => stderr.to_owned(),
            (false, false) => format!("{stdout}; {stderr}"),
            (true, true) => "<no output>".to_owned(),
        }
    }
}

pub fn run_probe(mut command: Command) -> Result<ProbeOutput, std::io::Error> {
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.try_clone()?))
        .stderr(Stdio::from(stderr.try_clone()?))
        .env("JOLTER_DOCTOR", "1");
    let mut child = command.spawn()?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (status, false);
        }
        if Instant::now() >= deadline {
            child.kill()?;
            break (child.wait()?, true);
        }
        thread::sleep(Duration::from_millis(25));
    };
    Ok(ProbeOutput {
        status,
        stdout: read_probe_output(&mut stdout)?,
        stderr: read_probe_output(&mut stderr)?,
        timed_out,
    })
}

fn read_probe_output(file: &mut fs::File) -> Result<String, std::io::Error> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(MAX_PROBE_OUTPUT).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[must_use]
pub fn extract_version(output: &str) -> Option<Version> {
    output
        .split(|character: char| character.is_whitespace() || character == ',' || character == ';')
        .map(|token| {
            token.trim_matches(|character: char| {
                !(character.is_ascii_alphanumeric()
                    || matches!(character, '.' | '-' | '+' | 'v' | 'V'))
            })
        })
        .find_map(|token| Version::parse(token.trim_start_matches(['v', 'V'])).ok())
}
