# Networking and Offline Mode

Jolter contacts official runtime providers and the npm registry to resolve
versions, download artifacts, and obtain integrity metadata.

## Provider Endpoints

The current provider model uses:

- official Node.js release metadata and archives;
- official Bun GitHub releases;
- official Deno GitHub releases;
- the official npm registry for npm, pnpm, and Yarn.

Only HTTPS artifact and metadata URLs are accepted. Redirects are validated and
must remain HTTPS.

## Timeouts and Retries

Network requests use finite connection and total timeouts. Transient failures
are retried at most three times with bounded backoff.

Retryable HTTP statuses are:

- `408 Request Timeout`
- `429 Too Many Requests`
- `500 Internal Server Error`
- `502 Bad Gateway`
- `503 Service Unavailable`
- `504 Gateway Timeout`

Permanent client errors are returned immediately.

## Proxy Configuration

Jolter uses standard proxy environment variables:

```text
HTTPS_PROXY
https_proxy
HTTP_PROXY
http_proxy
NO_PROXY
no_proxy
```

Proxy URLs should use `http://` or `https://`. Credentials embedded in proxy
URLs may be visible to process inspection and logs outside Jolter; prefer the
secret mechanism provided by the operating system or CI provider.

Example:

```bash
export HTTPS_PROXY="http://proxy.example.test:8080"
export NO_PROXY="localhost,127.0.0.1"
jolter doctor
```

PowerShell:

```powershell
$env:HTTPS_PROXY = "http://proxy.example.test:8080"
$env:NO_PROXY = "localhost,127.0.0.1"
jolter doctor
```

## Custom Certificate Authorities

Diagnostics recognize these common certificate variables:

```text
SSL_CERT_FILE
NODE_EXTRA_CA_CERTS
REQUESTS_CA_BUNDLE
```

Each configured path must point to an existing file. The exact trust behavior
also depends on the platform TLS stack and environment. Run `jolter doctor`
inside the same shell or CI step that performs installation.

Do not disable TLS verification to work around a certificate problem. Install
the correct organization CA and configure the environment explicitly.

## Cache Structure

The cache contains:

- provider metadata used for selector resolution;
- verified downloaded archives keyed by integrity identity;
- lock files used to serialize cache and installation activity.

Inspect it with:

```bash
jolter cache status
```

Remove metadata and downloads with:

```bash
jolter cache clean
```

Installed runtimes and tools are not removed by cache cleaning.

## Offline Mode

Enable offline mode with:

```bash
export JOLTER_OFFLINE=1
```

PowerShell:

```powershell
$env:JOLTER_OFFLINE = "1"
```

Offline mode forbids metadata requests and artifact downloads. The operation
succeeds only when the required metadata and verified archives are already
cached, or when an exact complete local installation can be reused without
metadata.

## Preparing an Offline Cache

On a connected machine with the same target operating system and architecture:

```bash
jolter sync
jolter doctor
jolter cache status
```

Preserve the reported cache directory. For CI, cache the Jolter cache
directory rather than assuming a runtime installation can be moved between
different operating systems or architectures.

Broad selectors such as `latest` and Node.js `lts` depend on cached metadata.
Exact versions reduce metadata requirements but still require the artifact
when the installation is absent.

## Diagnosing Network Failures

Run:

```bash
jolter doctor --no-progress
```

Check:

1. whether `JOLTER_OFFLINE` is unexpectedly enabled;
2. proxy URL syntax and reachability;
3. certificate file paths;
4. provider availability and rate limiting;
5. whether the cache was cleaned or restored for the wrong platform.

Verbose output adds transfer timing:

```bash
jolter sync --verbose --no-progress
```

Jolter never sends usage telemetry. Progress statistics are calculated
locally from downloaded byte counts and elapsed time.
