# CI Integration

Run from a project containing a supported requirement file:

```bash
jolter setup-ci
```

The command synchronizes the exact runtime and package manager, refreshes
shims, and reports the shims and cache directories.

Jolter detects CI and automatically disables in-place progress animation.
Stable action logs remain available on standard error. Explicit controls are:

```bash
jolter setup-ci --no-progress
jolter setup-ci --quiet
jolter setup-ci --json --no-color
```

Use `--quiet` when only the final result is wanted. Use `--no-progress` to keep
line-oriented action logs without carriage-return updates.

Machine-readable output is available:

```bash
jolter setup-ci --json
```

Jolter detects GitHub Actions, GitLab CI, CircleCI, Azure Pipelines,
Buildkite, and generic environments.

On GitHub Actions, `setup-ci` appends the shims directory to `GITHUB_PATH` and
writes these step outputs when `GITHUB_OUTPUT` is available:

- `runtime`
- `package_manager`
- `cache`

Cache the reported Jolter cache directory. Restoring the whole `JOLTER_HOME`
should be done only between jobs with the same operating system and
architecture.

The repository's scheduled provider-smoke workflow installs Node.js, Bun,
Deno, and pnpm from their real providers on every supported native runner.
