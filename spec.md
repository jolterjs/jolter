# Jolter Technical Specification

## Overview

Jolter is a modern JavaScript toolchain manager designed to provide a unified way to install, manage, pin, and synchronize JavaScript runtimes and development tools across projects.

Jolter aims to replace fragmented workflows involving tools such as:

* nvm
* fnm
* Volta
* Corepack
* manual runtime installation

while remaining focused on the JavaScript ecosystem.

---

# Goals

## Primary Goals

* Manage Node.js versions
* Manage Bun versions
* Manage Deno versions
* Manage package managers
* Automatically switch tool versions per project
* Provide reproducible development environments
* Simplify onboarding for developers

## Secondary Goals

* Toolchain health diagnostics
* Environment repair
* Team synchronization
* CI integration
* Cross-platform support

---

# Non Goals

Jolter is not:

* A package manager
* A build tool
* A bundler
* A framework
* A replacement for npm, pnpm, yarn, or Bun package installation

---

# Supported Platforms

* macOS
* Linux
* Windows
* WSL

---

# Architecture

The project must be implemented in Rust.

Architecture should be modular and scalable.

## Workspace Structure

```text
jolter/
├─ crates/
│  ├─ cli/
│  ├─ core/
│  ├─ config/
│  ├─ runtime/
│  ├─ installer/
│  ├─ resolver/
│  ├─ shim/
│  ├─ doctor/
│  └─ storage/
├─ tests/
├─ docs/
└─ scripts/
```

---

# Runtime Support

## Node.js

Supported:

```text
node@20
node@22
node@24
node@latest
node@lts
```

## Bun

Supported:

```text
bun@latest
bun@1
bun@1.3
```

## Deno

Supported:

```text
deno@latest
deno@2
```

---

# Configuration

Project configuration file:

```text
jolter.json
```

Example:

```json
{
  "schemaVersion": 1,
  "runtime": {
    "node": "24.x"
  },
  "packageManager": {
    "pnpm": "10.x"
  }
}
```

Future versions may support:

```json
{
  "runtime": {
    "bun": "1.x"
  }
}
```

---

# Commands

## Install And Activate A Tool

```bash
jolter use node@24
jolter use pnpm@10
```

Installs a runtime or managed package manager if missing and activates its
exact resolved version. Managed package managers require an active Node.js
runtime.

---

## Pin Runtime

```bash
jolter pin node@24
```

Writes project configuration.

---

## List Installed Versions

```bash
jolter list
```

Displays all installed runtimes.

---

## Doctor

```bash
jolter doctor
```

Checks:

* runtime version
* package manager version
* configuration validity
* missing installations

---

## Repair

```bash
jolter repair
```

Attempts to automatically resolve detected issues.

---

## Sync

```bash
jolter sync
```

Ensures local environment matches project requirements.

---

## Uninstall

```bash
jolter uninstall node@24.1.0
```

Removes an exact runtime or managed package manager version. Active versions
require explicit `--force`.

---

## Prune And Cache

```bash
jolter prune --dry-run
jolter cache status
jolter cache clean
```

Prune preserves active versions, current project requirements, and a
configurable number of newest complete versions.

---

## CI Setup

```bash
jolter setup-ci --json
```

Synchronizes exact project toolchains and reports resolved versions, shims,
and cache paths for automation.

---

## Terminal Output

Interactive runtime and package manager installation displays a single
auto-updating status line on standard error. Download status includes the
current action, artifact name, transferred bytes, percentage when known, and
transfer rate.

```text
[jolter] / fetch    node-v24.1.0-win-x64.zip [======>...........] 37% 18.1 MiB/48.2 MiB 8.4 MiB/s
```

Global output controls:

```bash
jolter use node@24 --no-progress
jolter sync --no-color
jolter setup-ci --quiet
jolter use pnpm@10 --verbose
```

Requirements:

* progress is written to standard error
* final command results remain on standard output
* JSON and completion output contain no decorative logging
* non-interactive and detected CI environments use stable line-oriented logs
* `--no-progress` disables in-place updates
* `--no-color` disables ANSI color
* `--quiet` suppresses operational logs while retaining command results
* `--verbose` includes transfer timing and rate detail
* `NO_COLOR`, `JOLTER_NO_COLOR`, and `JOLTER_NO_PROGRESS` are respected
* ANSI color is enabled only when the terminal advertises support; legacy
  Windows consoles automatically receive plain text
* tabular command output calculates column widths so status and path fields
  remain aligned without terminal-dependent tab stops

---

# Auto Switching

When entering a project directory:

```bash
cd project
```

Jolter should automatically detect:

```text
jolter.json
```

and activate the required runtime.

Project package manager requirements override globally active managed package
manager versions. When no project requirement applies, package manager shims
use the exact version activated by `jolter use`.

Priority:

1. jolter.json
2. package.json packageManager
3. .node-version
4. .nvmrc

---

# Storage Layout

Global storage:

```text
~/.jolter/
```

Structure:

```text
~/.jolter/
├─ runtimes/
│  ├─ node/
│  ├─ bun/
│  └─ deno/
├─ shims/
├─ cache/
└─ config/
```

---

# Shims

Jolter must use shim binaries.

Examples:

```text
node
npm
npx
pnpm
yarn
bun
deno
```

Shims resolve the correct version at runtime.

---

# Security

Requirements:

* HTTPS only
* SHA256 verification
* Checksum validation
* No execution of remote code
* No telemetry by default

---

# Telemetry

Default:

```text
disabled
```

No telemetry collection is implemented. Any future telemetry must be
explicitly opt-in and independently documented.

---

# Testing

Required coverage:

* Unit tests
* Integration tests
* Runtime installation tests
* Configuration parsing tests
* Shim resolution tests

Minimum target coverage:

```text
80%
```

---

# Future Features

## Toolchain Manifest

Extended configuration:

```json
{
  "runtime": {
    "node": "24.x"
  },
  "tools": {
    "pnpm": "10.x",
    "typescript": "5.x"
  }
}
```

---

## CI Support

```bash
jolter setup-ci
```

Automatically prepares CI environments.

---

## Plugin System

Future architecture must support:

```text
runtime plugins
tool plugins
custom resolvers
```

without requiring breaking changes.

---

# Versioning

Semantic Versioning:

```text
MAJOR.MINOR.PATCH
```

Examples:

```text
1.0.0
1.1.0
2.0.0
```

---

# Project Principles

* Fast
* Predictable
* Reproducible
* Minimal
* Cross-platform
* Developer-first
* No vendor lock-in
* Open Source
