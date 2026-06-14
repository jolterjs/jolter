# Command Reference

This reference describes the public CLI in Jolter `0.2.0`. Run
`jolter <command> --help` to inspect the syntax shipped by the installed
version.

## Global Options

Global options can be used with every command:

| Option | Purpose |
| --- | --- |
| `--no-progress` | Disable the in-place progress line and use stable action lines. |
| `--no-color` | Disable ANSI styling. |
| `-q`, `--quiet` | Suppress operational progress while keeping command results. |
| `-v`, `--verbose` | Include transfer timing and additional operational detail. |
| `-h`, `--help` | Print help. |
| `-V`, `--version` | Print the Jolter version. |

`--quiet` and `--verbose` conflict. CI detection, redirected output, and
`JOLTER_NO_PROGRESS` also disable the in-place renderer.

## Request Syntax

Runtime and tool requests use `<name>@<selector>`:

```text
node@lts
node@24
bun@1.2
deno@2.1.3
pnpm@10
yarn@4.x
npm@11.1.0
```

See [configuration](configuration.md) for selector and hash rules.

## `jolter setup`

```bash
jolter setup
jolter setup --shell powershell
jolter setup --shell cmd
jolter setup --shell bash
jolter setup --shell zsh
jolter setup --shell fish
```

Creates or refreshes self-shims and prints the exact `PATH` command for the
selected shell. `auto` is the default: PowerShell on Windows, otherwise a
supported shell inferred from `SHELL`.

The command does not silently modify a shell profile or persistent Windows
environment. See [shell setup](setup.md).

## `jolter use`

```bash
jolter use <runtime-or-tool>@<selector>
```

Examples:

```bash
jolter use node@lts
jolter use bun@latest
jolter use deno@2
jolter use pnpm@10
```

For a runtime, `use` resolves, installs or reuses, and globally activates the
exact version. For a managed tool, an installed and active Node.js runtime is
required. The tool's Node.js engine range is checked before activation.

Project requirements still take priority over a global selection when a shim
runs inside a configured project.

## `jolter pin`

```bash
jolter pin <runtime-or-tool>@<selector>
```

Writes `jolter.json` in the current directory. Pinning a runtime replaces the
project's previous runtime selection because schema version 1 allows one
runtime. Pinning a tool adds or replaces only that tool and preserves the
runtime and other tools.

`pin` records a requirement; it does not install it. Run `jolter sync`
afterward.

## `jolter update`

```bash
jolter update <runtime-or-tool>
jolter update <runtime-or-tool>@<selector>
jolter update --all
```

Examples:

```bash
jolter update node
jolter update pnpm
jolter update node@26
jolter update pnpm@latest
jolter update --all
```

A bare name updates the globally active version within its current major
release line. An explicit selector may stay on or move to another line and
does not require that kind to be active already. `--all` updates every active
runtime first, then every active tool. If nothing is active, `--all` is a
successful no-op.

Updating a tool requires an active Node.js runtime. Exact installed versions
can be reused without a metadata request; non-exact selectors consult provider
metadata to find the current best match.

`--all` is sequential rather than one cross-toolchain transaction. If a later
target fails, earlier successful updates remain active. Correct the failure and
rerun the command.

## `jolter list`

```bash
jolter list
jolter list --json
```

Lists every recognized runtime and tool version under `JOLTER_HOME`.

- `*` marks a globally active version.
- `ready` means the expected executable or tool entry point exists.
- `incomplete` means the version directory exists but is not usable.

JSON output has top-level `runtimes` and `tools` arrays. Each item includes
`kind`, `version`, `path`, `ready`, and `active`.

## `jolter doctor`

```bash
jolter doctor
jolter doctor --json
```

Runs non-destructive health checks for storage, platform support, project
resolution, installations, shims, `PATH`, cache state, proxies, and
certificate configuration.

Warnings do not fail the command. At least one failed check produces a nonzero
exit status. JSON output contains `healthy` and `checks`; each check has
`name`, `status`, `message`, and an optional `remediation`.

## `jolter sync`

```bash
jolter sync
```

Resolves the current project, installs or reuses its runtime and tools,
activates the exact results, and refreshes shims. A project runtime requirement
is mandatory. Configured tools require that runtime to be Node.js.

For broad selectors, Jolter may query current provider metadata. Set
`JOLTER_OFFLINE=1` only after the required metadata and archives have been
cached.

Each exact-version publication is atomic, but a project sync can make progress
before a later tool fails. Rerunning `sync` safely reuses completed compatible
installations.

## `jolter repair`

```bash
jolter repair
```

Runs the project synchronization flow in repair mode. Use it when an expected
version directory exists but its manifest, executable, entry point, or
integrity state is incomplete. Complete reusable installations are retained.

## `jolter uninstall`

Aliases: `remove`, `rm`.

```bash
jolter uninstall <runtime-or-tool>@<exact-version>
jolter uninstall <runtime-or-tool>@<exact-version> --force

jolter remove <runtime-or-tool>@<exact-version>
jolter remove <runtime-or-tool>@<exact-version> --force

jolter rm <runtime-or-tool>@<exact-version>
jolter rm <runtime-or-tool>@<exact-version> --force
```

Examples:

```bash
jolter uninstall node@24.5.0
jolter uninstall pnpm@10.12.1

jolter remove node@24.5.0
jolter remove pnpm@10.12.1

jolter rm node@24.5.0
jolter rm pnpm@10.12.1
```

Only exact semantic versions are accepted. Active versions are protected.
`--force` permits removal of an active version and clears that global
activation. Project files are not changed.

## `jolter prune`

```bash
jolter prune
jolter prune --dry-run
jolter prune --keep 2
```

Removes old and incomplete installations while protecting:

- every globally active runtime and tool;
- matching versions required by the current project;
- the newest `--keep` complete versions of every runtime and tool kind.

The default keep count is `1`. Always use `--dry-run` before changing retention
in automation.

Removal is sequential. A filesystem error can stop prune after earlier items
were removed; rerun the dry run to inspect the remaining plan.

## `jolter cache`

```bash
jolter cache status
jolter cache clean
```

`status` reports cached metadata and download file counts and size. `clean`
removes metadata and downloaded archives, but not installed versions, shims,
active selections, or lock files.

Cleaning the cache can remove offline readiness.

## `jolter setup-ci`

```bash
jolter setup-ci
jolter setup-ci --json
```

Synchronizes the project, refreshes shims, detects the CI provider, and reports
the resolved toolchain and cache paths. A project runtime is required.

On GitHub Actions it also appends the shims directory to `GITHUB_PATH` and
writes `runtime`, `tools`, and `cache` step outputs when `GITHUB_OUTPUT` is
available.

See [CI integration](ci.md) for full examples.

## `jolter completions`

Aliases: `c`, `comp`.

```bash
jolter completions bash
jolter completions zsh
jolter completions fish
jolter completions elvish
jolter completions powershell
```

Writes a completion script to standard output. Jolter does not install or
source the script automatically.

## Exit and Stream Contract

- Successful commands return exit code `0`.
- Parsing, installation, configuration, and failed health checks return a
  nonzero exit code.
- Final human results and JSON are written to standard output.
- progress and failures are written to standard error.
- machine-output commands suppress decorative progress.

See [output and automation](output-and-automation.md) before parsing human
output in scripts.
