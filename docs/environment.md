# Environment Variables

This reference separates public Jolter controls, CI integration variables,
standard networking variables, terminal detection, and internal child-process
state.

## Public Jolter Controls

| Variable | Values | Effect |
| --- | --- | --- |
| `JOLTER_HOME` | Non-empty path | Overrides the default `$HOME/.jolter` or `%USERPROFILE%\.jolter` root. |
| `JOLTER_OFFLINE` | `1`, `true`, or `yes` | Disables provider metadata requests and downloads. |
| `JOLTER_NO_PROGRESS` | `1`, `true`, or `yes` | Disables the updating progress line. |
| `JOLTER_NO_COLOR` | Any present value | Disables ANSI color. |
| `NO_COLOR` | Any present value | Standard ecosystem color disable. |

Boolean values are case-insensitive. Values other than `1`, `true`, or `yes`
do not enable offline or no-progress mode. Color variables follow the
presence-based `NO_COLOR` convention, so even an empty or `0` value disables
color when the variable exists.

## Storage

Set `JOLTER_HOME` consistently for setup, interactive commands, IDEs, and CI:

Bash:

```bash
export JOLTER_HOME="$HOME/.local/share/jolter"
```

PowerShell:

```powershell
$env:JOLTER_HOME = "$HOME\AppData\Local\Jolter"
```

Changing the value selects a different storage root, including shims, active
versions, installed toolchains, and cache.

## Offline and Output

Bash:

```bash
export JOLTER_OFFLINE=1
export JOLTER_NO_PROGRESS=1
export NO_COLOR=1
```

PowerShell:

```powershell
$env:JOLTER_OFFLINE = "1"
$env:JOLTER_NO_PROGRESS = "1"
$env:NO_COLOR = "1"
```

Command-line `--no-progress` and `--no-color` controls are useful for one
invocation. Environment variables are useful for a complete shell or CI job.

## CI Detection

These variables enable automated output mode when their value is `1`, `true`,
or `yes`:

```text
CI
GITHUB_ACTIONS
GITLAB_CI
CIRCLECI
TF_BUILD
BUILDKITE
```

Provider detection uses the specific provider variables. A generic `CI=true`
disables interactive progress but reports the provider as `generic` unless a
specific provider flag is also set.

## GitHub Actions Files

When `GITHUB_ACTIONS` is true, `jolter setup-ci` uses:

| Variable | Purpose |
| --- | --- |
| `GITHUB_PATH` | File that receives the Jolter shims directory. |
| `GITHUB_OUTPUT` | File that receives `runtime`, `tools`, and `cache` outputs. |

If either file variable is absent, Jolter skips that integration rather than
inventing a path.

## Proxy Variables

Standard proxy variables:

```text
HTTPS_PROXY
https_proxy
HTTP_PROXY
http_proxy
NO_PROXY
no_proxy
```

Use `http://` or `https://` proxy URLs. Protect embedded credentials and prefer
the secret-injection mechanism provided by the automation platform.

## Certificate Variables

Doctor validates configured paths for:

```text
SSL_CERT_FILE
NODE_EXTRA_CA_CERTS
REQUESTS_CA_BUNDLE
```

Trust behavior depends on the platform and TLS stack. A configured path must
exist and should contain the intended organization CA material.

## Shell and Command Resolution

Jolter reads standard environment state:

| Variable | Purpose |
| --- | --- |
| `PATH` | Finds Jolter, evaluates shim precedence, and constructs child process paths. |
| `SHELL` | Helps `jolter setup --shell auto` choose Bash, Zsh, or Fish. |
| `PATHEXT` | Supports executable discovery on Windows. |
| `HOME` | Default home discovery on Unix. |
| `USERPROFILE` | Default home discovery on Windows. |

## Terminal Capability Detection

Jolter may inspect `WT_SESSION`, `ANSICON`, `ConEmuANSI`, `TERM`,
`TERM_PROGRAM`, and `COLORTERM` when deciding whether a Windows or Unix
terminal can render ANSI output safely.

These are detection inputs, not Jolter-specific configuration. Use
`--no-color` when a terminal wrapper reports incorrect capability.

## Internal Variables

Jolter sets these for child processes:

| Variable | Meaning |
| --- | --- |
| `JOLTER_RUNTIME_ROOT` | Root of the runtime selected by a shim. |
| `JOLTER_DOCTOR` | Marks a bounded `--version` probe launched by doctor. |

They are not user configuration interfaces and may change outside the public
compatibility contract. Do not set them to influence resolution.

## Diagnostics

After changing environment variables:

```bash
jolter doctor
```

Run doctor from the same terminal, IDE, service account, or CI step that
experiences the problem. Parent processes can retain older environment values
until they are restarted.
