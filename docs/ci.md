# CI Integration

Jolter uses the same project declaration in development and CI. The production
entry point is:

```bash
jolter setup-ci
```

It resolves the current project, installs or reuses the runtime and all
configured tools, activates the exact results, refreshes shims, detects the CI
provider, and reports paths needed by later steps.

## CI Requirements

The repository must provide a runtime requirement through `jolter.json`,
`.node-version`, or `.nvmrc`. Tools can come from `jolter.json#tools` or the
standard `package.json#packageManager` compatibility field.

For deterministic builds, prefer exact versions or intentionally controlled
release lines. `latest` and `lts` can resolve differently as providers publish
new releases.

## Stable Logs

CI detection automatically disables the in-place progress renderer:

```bash
jolter setup-ci --no-progress
```

Useful modes:

```bash
jolter setup-ci --quiet
jolter setup-ci --json --no-color
jolter setup-ci --verbose --no-progress
```

Use `--no-progress` for readable action logs, `--quiet` for final results only,
and JSON when a later step consumes resolution data.

## Machine Output

```bash
jolter setup-ci --json
```

The JSON object contains:

- detected `provider`;
- exact `runtime` kind, version, and path;
- exact `tools` kind, version, and path array;
- `shims` directory;
- `cache` directory.

See [output and automation](output-and-automation.md) for the complete shape.

## GitHub Actions

Example:

```yaml
name: Test

on:
  push:
  pull_request:

jobs:
  test:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v6

      - name: Restore Jolter cache
        uses: actions/cache@v4
        with:
          path: ${{ runner.temp }}/jolter-home/cache
          key: jolter-${{ runner.os }}-${{ runner.arch }}-${{ hashFiles('jolter.json', '.node-version', '.nvmrc', 'package.json') }}

      - name: Install and synchronize Jolter
        id: jolter
        env:
          JOLTER_HOME: ${{ runner.temp }}/jolter-home
        run: |
          curl -fsSL https://jolter.dev/unix/install | sh
          jolter setup-ci --no-progress

      - name: Test
        env:
          JOLTER_HOME: ${{ runner.temp }}/jolter-home
        run: |
          node --version
          pnpm --version
          pnpm test
```

On GitHub Actions, `setup-ci`:

- appends the shims directory to `GITHUB_PATH`;
- writes `runtime` to `GITHUB_OUTPUT`;
- writes comma-separated `tools` when tools are configured;
- writes the cache path as `cache`.

The outputs are available from later steps through the step id:

```yaml
- run: echo '${{ steps.jolter.outputs.runtime }}'
```

Keep `JOLTER_HOME` consistent in later steps. `GITHUB_PATH` carries the shim
path forward, while storage discovery still depends on the same home.

For Windows runners, install in PowerShell:

```yaml
- name: Install and synchronize Jolter
  shell: pwsh
  env:
    JOLTER_HOME: ${{ runner.temp }}\jolter-home
  run: |
    irm https://jolter.dev/win/install | iex
    jolter setup-ci --no-progress
```

## GitLab CI

```yaml
variables:
  JOLTER_HOME: "$CI_PROJECT_DIR/.jolter-ci"

cache:
  key: "jolter-$CI_RUNNER_EXECUTABLE_ARCH"
  paths:
    - .jolter-ci/cache/

test:
  script:
    - curl -fsSL https://jolter.dev/unix/install | sh
    - jolter setup-ci --no-progress
    - export PATH="$JOLTER_HOME/shims:$PATH"
    - node --version
    - pnpm test
```

GitLab is detected and reported, but Jolter does not mutate GitLab job
variables. Add the shims directory to `PATH` in the job.

## Other Providers

Jolter identifies CircleCI, Azure Pipelines, Buildkite, and generic CI
environments. Only GitHub Actions receives provider-specific path and output
file integration in the current release. For other providers:

1. set a stable `JOLTER_HOME`;
2. restore `$JOLTER_HOME/cache`;
3. install Jolter;
4. run `jolter setup-ci --json` or `--no-progress`;
5. prepend `$JOLTER_HOME/shims` to later-step `PATH`.

## Cache Strategy

Cache:

```text
$JOLTER_HOME/cache
```

Include operating system and architecture in the cache key. Include project
requirement files in the key or restore-key strategy.

Caching the whole `JOLTER_HOME` can be faster but is less portable and includes
installed executables and active selections. Do it only between jobs with the
same operating system, architecture, trust boundary, and Jolter compatibility
line.

`jolter cache clean` removes the data needed for offline provider resolution.

## Offline CI

Set `JOLTER_OFFLINE=1` only after restoring a cache containing all required
metadata and artifacts:

```yaml
env:
  JOLTER_OFFLINE: "1"
```

Exact project versions simplify cache preparation. Broad selectors still need
appropriate cached metadata.

## Security Guidance

- Pin third-party actions by policy, ideally to immutable commits.
- Treat installer scripts as remote code and use an approved mirror or
  checksum-verified release archive where required.
- Do not share writable caches across untrusted branches or repositories.
- Keep credentials out of proxy URLs when the CI platform provides secret
  injection.
- Preserve standard TLS verification.

## Failure Triage

When setup fails:

```bash
jolter doctor --json --no-color
jolter cache status
```

Upload the JSON report and relevant line-oriented logs as artifacts, but review
paths and environment-derived details according to organization policy.
