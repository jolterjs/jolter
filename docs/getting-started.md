# Getting Started

This guide takes a new machine from no Jolter installation to a project that
automatically uses its declared JavaScript toolchain.

## 1. Install Jolter

Windows PowerShell:

```powershell
irm https://jolter.dev/win/install | iex
```

Linux or macOS:

```bash
curl -fsSL https://jolter.dev/unix/install | sh
```

For checksum verification, manual archives, or source builds, use the
[installation guide](installation.md).

## 2. Enable Automatic Switching

```bash
jolter setup
```

Jolter prints commands for the current shell. Apply the persistent command,
restart the terminal, and confirm that the shims directory appears before any
other Node.js manager on `PATH`.

```bash
jolter doctor
```

The shell guide explains [PATH setup and command shims](setup.md) in detail.

## 3. Choose a Runtime

Install the current Node.js LTS release:

```bash
jolter use node@lts
node --version
```

`use` both installs and globally activates the resolved exact version. You can
also select a release line or exact version:

```bash
jolter use node@24
jolter use node@24.5.0
jolter use bun@1
jolter use deno@2
```

## 4. Choose Project Tools

Managed npm, pnpm, and Yarn releases require an active Node.js runtime:

```bash
jolter use pnpm@10
jolter use yarn@4
```

Jolter checks each tool's declared Node.js engine range before using it.

## 5. Pin a Project

Run from the project directory:

```bash
jolter pin node@24
jolter pin pnpm@10
jolter pin yarn@4
```

The commands create or update `jolter.json` without removing other pinned
tools:

```json
{
  "schemaVersion": 1,
  "runtime": {
    "node": "24"
  },
  "tools": {
    "pnpm": "10",
    "yarn": "4"
  }
}
```

Commit this file so other developers and CI resolve the same release lines.

## 6. Reproduce the Project

```bash
jolter sync
```

`sync` resolves the project requirements, installs missing versions, reuses
complete compatible installations, activates the results, and refreshes
shims.

Verify the finished environment:

```bash
jolter doctor
node --version
pnpm --version
```

## 7. Move Between Projects

No shell hook is required. Each generated command shim resolves requirements
from the current directory whenever it runs. A project requirement wins over
the globally active version; outside a configured project, the global
selection from `jolter use` is used.

## 8. Add CI

Use the same project declaration in automation:

```bash
jolter setup-ci --no-progress
```

For provider examples, cache strategy, and JSON outputs, continue with
[CI integration](ci.md).

## Next Steps

- Learn every selector and field in [configuration](configuration.md).
- Understand precedence in [project resolution](project-resolution.md).
- Review all operations in the [command reference](commands.md).
- Use [troubleshooting](troubleshooting.md) when `doctor` reports a problem.
