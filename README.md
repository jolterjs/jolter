# Jolter

**One fast, reliable home for your JavaScript runtimes and tools.**

Jolter keeps Node.js, Bun, Deno, npm, pnpm, and Yarn consistent across your
machine, your team, and CI. Pin what a project needs once, enter the directory,
and use the right toolchain without manually switching versions.

## Install

### Windows

Run in PowerShell:

```powershell
irm https://jolter.dev/install.ps1 | iex
```

### Linux and macOS

Run in a terminal:

```bash
curl -fsSL https://jolter.dev/install.sh | sh
```

Then finish shell setup:

```bash
jolter setup
```

Apply the command Jolter prints, restart your terminal, and verify everything:

```bash
jolter doctor
```

See the [installation guide](docs/installation.md) for manual installation,
checksums, source builds, upgrades, and platform details.

## Your Toolchain, Ready

Install the runtime you want:

```bash
jolter use node@lts
```

Add the tools you prefer:

```bash
jolter use pnpm@10
jolter use yarn@4
```

Pin the project so everyone gets the same setup:

```bash
cd my-project
jolter pin node@24
jolter pin pnpm@10
jolter sync
```

Jolter creates a small `jolter.json` file that can be committed with the
project. Team members and CI can then reproduce the toolchain with one command.

## Why Jolter

### Stop switching versions by hand

Jolter automatically selects project versions through lightweight command
shims. Move between repositories and `node`, `pnpm`, `yarn`, `bun`, and `deno`
follow the project you are working in.

### Use one workflow everywhere

The same commands work on Windows, macOS, Linux, and WSL. Jolter understands
its own project file as well as `.node-version`, `.nvmrc`, and
`package.json#packageManager`.

### Keep local development and CI aligned

`jolter sync` prepares a developer machine. `jolter setup-ci` prepares an
automation job and exposes the resolved runtime, tools, shims, and cache
location.

### Know when something is wrong

`jolter doctor` checks the project, installed versions, command routing,
storage, network configuration, and offline readiness, then gives practical
remediation steps.

### Install with confidence

Jolter downloads from official providers, verifies release integrity, rejects
insecure redirects, extracts into temporary locations, and publishes complete
installations atomically.

## Supported Toolchains

| Category  | Supported                                           |
| --------- | --------------------------------------------------- |
| Runtimes  | Node.js, Bun, Deno                                  |
| Tools     | npm, pnpm, Yarn                                     |
| Platforms | Windows x64, Linux x64, macOS x64, macOS ARM64, WSL |
| Shells    | PowerShell, Command Prompt, Bash, Zsh, Fish         |

Tools run through the selected Node.js version, so Jolter also checks their
declared Node.js compatibility before activation.

## Project Configuration

```json
{
  "$schema": "https://schemas.jolter.dev/project/v2/schema.json",
  "schemaVersion": 2,
  "runtime": {
    "node": "24.x"
  },
  "tools": {
    "pnpm": "10.x",
    "yarn": "4.x"
  }
}
```

A project can select one runtime and multiple tools. Selectors can describe an
exact version, a release line such as `24` or `24.x`, `latest`, or Node.js
`lts`.

Learn more in [configuration](docs/configuration.md) and
[project resolution](docs/project-resolution.md).

## Everyday Commands

```bash
# Install and activate
jolter use node@lts
jolter use pnpm@10

# Save project requirements
jolter pin node@24
jolter pin pnpm@10

# Reproduce or repair a project
jolter sync
jolter repair

# Update active versions
jolter update node
jolter update --all

# Inspect health and installed versions
jolter doctor
jolter list

# Safely reclaim space
jolter prune --dry-run
jolter cache status
```

See the [complete command reference](docs/commands.md) for every command,
argument, output mode, and exit behavior.

## Built for Automation

Jolter automatically switches to stable line-oriented logs when output is
redirected or a CI environment is detected.

```bash
jolter setup-ci --no-progress
jolter setup-ci --json --no-color
```

Use `--quiet` for final results only, `--verbose` for transfer details, and
`--no-progress` for deterministic logs. Machine-readable output is available
for inventory, diagnostics, and CI setup.

Read the [CI guide](docs/ci.md) and
[output and automation reference](docs/output-and-automation.md).

## Documentation

Start at the [documentation home](docs/README.md).

- [Getting started](docs/getting-started.md)
- [Installation](docs/installation.md)
- [Command reference](docs/commands.md)
- [Configuration](docs/configuration.md)
- [Runtimes and tools](docs/runtimes-and-tools.md)
- [Shell setup and automatic switching](docs/setup.md)
- [CI integration](docs/ci.md)
- [Environment variables](docs/environment.md)
- [Diagnostics](docs/diagnostics.md)
- [Troubleshooting](docs/troubleshooting.md)
- [Security model](docs/security.md)
- [Architecture](docs/architecture.md)

## Project Status

Jolter `0.2.0` provides the stable core command surface and remains pre-1.0
while its cross-platform release and real-provider workflows complete a
release-candidate period. Compatibility commitments and known release work are
documented in the [compatibility policy](docs/compatibility.md) and
[roadmap](ROADMAP.md).

Jolter is open source under the [MIT License](LICENSE).
