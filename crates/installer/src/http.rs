use std::{
    fs::File,
    io::{BufWriter, Read, Write},
    path::Path,
    thread,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use jolter_runtime::{ToolHash, ToolHashAlgorithm};
use reqwest::{
    StatusCode, Url,
    blocking::Client,
    header::{ACCEPT, RETRY_AFTER},
    redirect::{Attempt, Policy},
};
use sha1::Sha1;
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512};

use crate::{
    MAX_ARCHIVE_BYTES, MAX_HTTP_ATTEMPTS, MAX_METADATA_BYTES, MAX_RETRY_AFTER,
    error::InstallerError,
    progress::{ProgressAction, ProgressEvent, ProgressReporter},
};

pub trait HttpClient: Send + Sync {
    fn get_text(&self, url: &str) -> Result<String, InstallerError>;
    fn get_npm_metadata(&self, url: &str) -> Result<String, InstallerError> {
        self.get_text(url)
    }
    fn download(
        &self,
        url: &str,
        destination: &Path,
        name: &str,
        reporter: &dyn ProgressReporter,
    ) -> Result<(), InstallerError>;
}

#[derive(Debug, Clone)]
pub struct ReqwestHttpClient {
    client: Client,
}

fn github_token_from_env() -> Option<String> {
    std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .or_else(|_| std::env::var("JOLTER_GITHUB_TOKEN"))
        .ok()
        .filter(|token| !token.trim().is_empty())
}

impl ReqwestHttpClient {
    pub fn new() -> Result<Self, InstallerError> {
        let client = Client::builder()
            .user_agent(concat!("jolter/", env!("CARGO_PKG_VERSION")))
            .tcp_nodelay(true)
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(30 * 60))
            .redirect(Policy::custom(https_redirect_policy))
            .build()
            .map_err(InstallerError::HttpClient)?;
        Ok(Self { client })
    }

    fn response(
        &self,
        url: &str,
        accept: Option<&str>,
    ) -> Result<reqwest::blocking::Response, InstallerError> {
        ensure_https(url)?;
        for attempt in 0..MAX_HTTP_ATTEMPTS {
            let mut request = self.client.get(url);
            if let Some(accept) = accept {
                request = request.header(ACCEPT, accept);
            }
            if (url.contains("github.com") || url.contains("api.github.com"))
                && let Some(token) = github_token_from_env()
            {
                request = request.header("Authorization", format!("Bearer {token}"));
            }
            match request.send() {
                Ok(response) if is_rate_limit_or_retryable(&response, attempt) => {
                    let delay = retry_delay_for_response(attempt, &response);
                    thread::sleep(delay);
                }
                Ok(response) => {
                    let response =
                        response
                            .error_for_status()
                            .map_err(|source| InstallerError::Http {
                                url: url.to_owned(),
                                source,
                            })?;
                    ensure_https(response.url().as_str())?;
                    return Ok(response);
                }
                Err(source)
                    if retryable_request_error(&source) && attempt + 1 < MAX_HTTP_ATTEMPTS =>
                {
                    thread::sleep(retry_delay(attempt, None));
                }
                Err(source) => {
                    return Err(InstallerError::Http {
                        url: url.to_owned(),
                        source,
                    });
                }
            }
        }
        unreachable!("the bounded HTTP attempt loop always returns on its final attempt")
    }
}

pub(crate) fn is_rate_limit_or_retryable(
    response: &reqwest::blocking::Response,
    attempt: usize,
) -> bool {
    if attempt + 1 >= MAX_HTTP_ATTEMPTS {
        return false;
    }
    let status = response.status();
    if retryable_status(status) {
        return true;
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return true;
    }
    if status == StatusCode::FORBIDDEN {
        let headers = response.headers();
        if let Some(remaining) = headers
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
        {
            if remaining == "0" {
                return true;
            }
        }
        if headers.contains_key(RETRY_AFTER) || headers.contains_key("x-ratelimit-reset") {
            return true;
        }
    }
    false
}

#[must_use]
pub(crate) fn retry_delay_for_response(
    attempt: usize,
    response: &reqwest::blocking::Response,
) -> Duration {
    let headers = response.headers();
    if let Some(seconds) = headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        return Duration::from_secs(seconds).min(MAX_RETRY_AFTER);
    }
    if let Some(reset_secs) = headers
        .get("x-ratelimit-reset")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if reset_secs > now {
            let wait = reset_secs - now;
            return Duration::from_secs(wait).min(MAX_RETRY_AFTER);
        }
    }
    retry_delay(attempt, None)
}

pub(crate) fn retryable_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::INTERNAL_SERVER_ERROR
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    )
}

fn retryable_request_error(error: &reqwest::Error) -> bool {
    error.is_connect() || error.is_timeout() || error.is_request()
}

pub(crate) fn retry_delay(
    attempt: usize,
    retry_after: Option<&reqwest::header::HeaderValue>,
) -> Duration {
    if let Some(seconds) = retry_after
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        return Duration::from_secs(seconds).min(MAX_RETRY_AFTER);
    }
    Duration::from_millis(250 * (1_u64 << attempt.min(4)))
}

impl HttpClient for ReqwestHttpClient {
    fn get_text(&self, url: &str) -> Result<String, InstallerError> {
        read_text_response(self.response(url, None)?, url)
    }

    fn get_npm_metadata(&self, url: &str) -> Result<String, InstallerError> {
        read_text_response(
            self.response(url, Some("application/vnd.npm.install-v1+json"))?,
            url,
        )
    }

    fn download(
        &self,
        url: &str,
        destination: &Path,
        name: &str,
        reporter: &dyn ProgressReporter,
    ) -> Result<(), InstallerError> {
        reporter.report(ProgressEvent::Stage {
            action: ProgressAction::Connect,
            target: name,
        });
        let mut response = self.response(url, None)?;
        let total = response.content_length();
        if total.is_some_and(|length| length > MAX_ARCHIVE_BYTES) {
            return Err(InstallerError::ArtifactTooLarge {
                url: url.to_owned(),
            });
        }
        reporter.report(ProgressEvent::DownloadStarted { name, total });
        let file = File::create(destination).map_err(InstallerError::Io)?;
        let mut writer = BufWriter::with_capacity(256 * 1024, file);
        let mut downloaded = 0_u64;
        let mut buffer = vec![0_u8; 256 * 1024];
        loop {
            let read = response.read(&mut buffer).map_err(InstallerError::Io)?;
            if read == 0 {
                break;
            }
            downloaded = downloaded.saturating_add(read as u64);
            if downloaded > MAX_ARCHIVE_BYTES {
                return Err(InstallerError::ArtifactTooLarge {
                    url: url.to_owned(),
                });
            }
            writer
                .write_all(&buffer[..read])
                .map_err(InstallerError::Io)?;
            reporter.report(ProgressEvent::DownloadAdvanced {
                name,
                downloaded,
                total,
            });
        }
        writer.flush().map_err(InstallerError::Io)?;
        reporter.report(ProgressEvent::DownloadFinished {
            name,
            downloaded,
            total,
        });
        Ok(())
    }
}

fn read_text_response(
    response: reqwest::blocking::Response,
    url: &str,
) -> Result<String, InstallerError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_METADATA_BYTES)
    {
        return Err(InstallerError::MetadataTooLarge {
            url: url.to_owned(),
        });
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(InstallerError::Io)?;
    if bytes.len() as u64 > MAX_METADATA_BYTES {
        return Err(InstallerError::MetadataTooLarge {
            url: url.to_owned(),
        });
    }
    String::from_utf8(bytes).map_err(|source| InstallerError::InvalidUtf8 {
        url: url.to_owned(),
        source,
    })
}

fn https_redirect_policy(attempt: Attempt<'_>) -> reqwest::redirect::Action {
    if attempt.previous().len() >= 10 {
        return attempt.error("too many redirects");
    }
    if attempt.url().scheme() != "https" {
        return attempt.error("refusing redirect to a non-HTTPS URL");
    }
    attempt.follow()
}

pub(crate) fn ensure_https(value: &str) -> Result<(), InstallerError> {
    let url = Url::parse(value).map_err(|_| InstallerError::InvalidUrl(value.to_owned()))?;
    if url.scheme() != "https" {
        return Err(InstallerError::InsecureUrl(value.to_owned()));
    }
    Ok(())
}

pub(crate) fn offline_mode() -> bool {
    std::env::var_os("JOLTER_OFFLINE").is_some_and(|value| {
        matches!(
            value.to_string_lossy().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        )
    })
}

pub(crate) fn write_cache_file(path: &Path, contents: &[u8]) -> Result<(), InstallerError> {
    let parent = path
        .parent()
        .ok_or_else(|| InstallerError::MetadataCacheRead {
            path: path.to_path_buf(),
        })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(InstallerError::Io)?;
    temporary.write_all(contents).map_err(InstallerError::Io)?;
    temporary
        .persist(path)
        .map_err(|error| InstallerError::Io(error.error))?;
    Ok(())
}

pub(crate) fn validate_checksum(value: &str) -> Result<(), InstallerError> {
    if value.len() != 64 || !value.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(InstallerError::InvalidChecksum(value.to_owned()));
    }
    Ok(())
}

pub(crate) fn parse_github_digest(value: &str) -> Option<String> {
    let checksum = value.strip_prefix("sha256:")?;
    validate_checksum(checksum).ok()?;
    Some(checksum.to_ascii_lowercase())
}

pub(crate) fn checksum_for(contents: &str, file_name: &str) -> Result<String, InstallerError> {
    for line in contents.lines() {
        let mut fields = line.split_whitespace();
        let Some(checksum) = fields.next() else {
            continue;
        };
        let Some(name) = fields.next() else {
            continue;
        };
        if name.trim_start_matches('*') == file_name {
            validate_checksum(checksum)?;
            return Ok(checksum.to_ascii_lowercase());
        }
    }
    Err(InstallerError::ChecksumNotFound {
        file: file_name.to_owned(),
    })
}

pub(crate) fn parse_checksum_value(contents: &str) -> Result<String, InstallerError> {
    let checksum = contents
        .split_whitespace()
        .next()
        .ok_or(InstallerError::EmptyChecksum)?;
    validate_checksum(checksum)?;
    Ok(checksum.to_ascii_lowercase())
}

pub fn verify_sha256(path: &Path, expected: &str) -> Result<(), InstallerError> {
    validate_checksum(expected)?;
    let mut file = File::open(path).map_err(InstallerError::Io)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 256 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(InstallerError::Io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(InstallerError::ChecksumMismatch {
            expected: expected.to_owned(),
            actual,
        })
    }
}

pub(crate) fn verify_sha512(path: &Path, expected: &str) -> Result<(), InstallerError> {
    let expected_bytes = BASE64
        .decode(expected)
        .map_err(|_| InstallerError::InvalidIntegrity(format!("sha512-{expected}")))?;
    if expected_bytes.len() != 64 {
        return Err(InstallerError::InvalidIntegrity(format!(
            "sha512-{expected}"
        )));
    }
    let mut file = File::open(path).map_err(InstallerError::Io)?;
    let mut hasher = Sha512::new();
    let mut buffer = vec![0_u8; 256 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(InstallerError::Io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual_bytes = hasher.finalize();
    if actual_bytes.as_slice() == expected_bytes {
        Ok(())
    } else {
        Err(InstallerError::ChecksumMismatch {
            expected: format!("sha512-{expected}"),
            actual: format!("sha512-{}", BASE64.encode(actual_bytes)),
        })
    }
}

pub(crate) fn verify_tool_hash(path: &Path, expected: &ToolHash) -> Result<(), InstallerError> {
    let actual = match expected.algorithm {
        ToolHashAlgorithm::Sha1 => digest_hex::<Sha1>(path)?,
        ToolHashAlgorithm::Sha224 => digest_hex::<Sha224>(path)?,
        ToolHashAlgorithm::Sha256 => digest_hex::<Sha256>(path)?,
        ToolHashAlgorithm::Sha384 => digest_hex::<Sha384>(path)?,
        ToolHashAlgorithm::Sha512 => digest_hex::<Sha512>(path)?,
    };
    if actual.eq_ignore_ascii_case(&expected.value) {
        Ok(())
    } else {
        Err(InstallerError::ToolHashMismatch {
            algorithm: expected.algorithm,
            expected: expected.value.clone(),
            actual,
        })
    }
}

fn digest_hex<D: Digest>(path: &Path) -> Result<String, InstallerError> {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";

    let mut file = File::open(path).map_err(InstallerError::Io)?;
    let mut hasher = D::new();
    let mut buffer = vec![0_u8; 256 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(InstallerError::Io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let output = hasher.finalize();
    let mut hex = String::with_capacity(output.len() * 2);
    for byte in output {
        hex.push(char::from(DIGITS[usize::from(byte >> 4)]));
        hex.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    Ok(hex)
}
