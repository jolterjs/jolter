# Output and Automation

Jolter separates human presentation, progress events, final results, and
machine-readable data so the CLI remains pleasant interactively and
predictable in automation.

## Render Modes

### Interactive

When standard error is a terminal and automation is not detected, long
operations use one updating status line. Downloads can show current action,
bytes, percentage, and transfer rate without filling the terminal history.

### Plain

Redirected output, CI, `--no-progress`, or `JOLTER_NO_PROGRESS=1` switches to
stable line-oriented events. No carriage-return animation is emitted.

### Quiet

`--quiet` suppresses operational events while retaining final command results
and failures.

### Machine

Commands with a JSON or generated-script output mode suppress decorative
progress so standard output contains only the requested payload.

## Global Controls

```bash
jolter sync --no-progress
jolter list --no-color
jolter setup-ci --quiet
jolter use node@24 --verbose
```

| Control                | Effect                                           |
| ---------------------- | ------------------------------------------------ |
| `--no-progress`        | Stable action lines instead of an updating line. |
| `--no-color`           | No ANSI color sequences.                         |
| `--quiet`              | Final results and failures only.                 |
| `--verbose`            | Transfer timing, rate, and additional detail.    |
| `JOLTER_NO_PROGRESS=1` | Environment equivalent of plain progress.        |
| `JOLTER_NO_COLOR=1`    | Jolter-specific color disable.                   |
| `NO_COLOR`             | Standard ecosystem color disable.                |

`JOLTER_NO_PROGRESS` and CI flags recognize `1`, `true`, and `yes`,
case-insensitively. `JOLTER_NO_COLOR` and `NO_COLOR` disable color whenever
the variable is present.

## CI Detection

Interactive progress is disabled when any recognized flag is true:

- `CI`
- `GITHUB_ACTIONS`
- `GITLAB_CI`
- `CIRCLECI`
- `TF_BUILD`
- `BUILDKITE`

This detection affects presentation only. It does not alter version
resolution, integrity checks, installation paths, or command results.

## Stream Contract

- Standard output contains final results, tables, instructions, JSON, and
  generated completion scripts.
- Standard error contains progress events and failures.
- Machine output keeps standard output free from decorative logging.

Scripts should parse JSON rather than human messages. Human wording and
styling may improve between compatible releases.

## Inventory JSON

```bash
jolter list --json
```

Shape:

```json
{
  "runtimes": [
    {
      "active": true,
      "kind": "node",
      "path": "/home/user/.jolter/runtimes/node/24.5.0",
      "ready": true,
      "version": "24.5.0"
    }
  ],
  "tools": [
    {
      "active": true,
      "kind": "pnpm",
      "path": "/home/user/.jolter/tools/pnpm/10.12.1",
      "ready": true,
      "version": "10.12.1"
    }
  ]
}
```

Array order is stable by tool kind and semantic version in the current
implementation, but consumers should identify records by fields rather than
position.

## Doctor JSON

```bash
jolter doctor --json
```

Shape:

```json
{
  "checks": [
    {
      "message": "using /home/user/.jolter",
      "name": "storage",
      "status": "pass"
    }
  ],
  "healthy": true
}
```

`status` is `pass`, `warning`, or `fail`. A check may include `remediation`.
`healthy` is false only when at least one check fails.

## CI Setup JSON

```bash
jolter setup-ci --json
```

Shape:

```json
{
  "cache": "/home/user/.jolter/cache",
  "provider": "github-actions",
  "runtime": {
    "kind": "node",
    "path": "/home/user/.jolter/runtimes/node/24.5.0",
    "version": "24.5.0"
  },
  "shims": "/home/user/.jolter/shims",
  "tools": [
    {
      "kind": "pnpm",
      "path": "/home/user/.jolter/tools/pnpm/10.12.1",
      "version": "10.12.1"
    }
  ]
}
```

Provider values currently include `github-actions`, `gitlab-ci`, `circleci`,
`azure-pipelines`, `buildkite`, and `generic`.

## GitHub Actions Outputs

When the runner supplies the standard files, `setup-ci` writes:

| Output    | Example                      |
| --------- | ---------------------------- |
| `runtime` | `node@24.5.0`                |
| `tools`   | `pnpm@10.12.1,yarn@4.9.2`    |
| `cache`   | `/home/runner/.jolter/cache` |

The shims directory is appended to `GITHUB_PATH`.

## Exit Status

Use the process exit code as the primary success signal:

- `0`: operation completed, including `doctor` with warnings and
  `update --all` with no active versions;
- nonzero: command-line validation, resolution, installation, storage,
  environment, or failed doctor checks.

Do not infer success from an empty standard-error stream.

## Logging Recommendations

For normal CI logs:

```bash
jolter setup-ci --no-progress
```

For data-driven automation:

```bash
jolter setup-ci --json --no-color
```

For minimal release logs:

```bash
jolter setup-ci --quiet --no-color
```

Avoid merging standard output and standard error before parsing JSON.
