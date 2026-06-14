# Jolter Documentation

This documentation covers Jolter from first installation through production
automation, maintenance, security review, and release engineering.

## Start Here

- [Getting started](getting-started.md): install Jolter and prepare a project.
- [Installation](installation.md): scripts, archives, checksums, source builds,
  upgrades, and platform support.
- [Command reference](commands.md): every command, flag, and operational
  behavior.
- [Configuration](configuration.md): the complete `jolter.json` schema and
  selector rules.

## Daily Use

- [Runtimes and tools](runtimes-and-tools.md): supported providers, version
  selection, activation, and Node.js compatibility.
- [Project resolution](project-resolution.md): requirement precedence,
  directory discovery, global fallback, and automatic switching.
- [Shell setup](setup.md): shims, `PATH`, shell persistence, and completions.
- [Diagnostics and inventory](diagnostics.md): `list`, `doctor`, JSON output,
  and health interpretation.
- [Storage lifecycle](lifecycle.md): uninstall, prune, cache maintenance, and
  storage layout.
- [Troubleshooting](troubleshooting.md): symptom-based recovery procedures.

## Automation and Operations

- [CI integration](ci.md): reproducible jobs and provider-specific behavior.
- [Output and automation](output-and-automation.md): progress, colors, streams,
  JSON contracts, environment controls, and exit codes.
- [Environment variables](environment.md): storage, offline mode, output, CI,
  proxy, certificate, shell, and internal process variables.
- [Networking and offline mode](networking-and-offline.md): proxies,
  certificate configuration, caching, retries, and disconnected operation.
- [Security model](security.md): trust boundaries, integrity checks, safe
  extraction, locking, and disclosure guidance.
- [Compatibility policy](compatibility.md): configuration, storage, platform,
  and pre-1.0 guarantees.

## Maintainers

- [Architecture](architecture.md): crate boundaries and end-to-end flows.
- [Development](development.md): local setup, quality gates, tests, and
  contribution workflow.
- [Release operations](releasing.md): release preparation, CI evidence,
  artifacts, checksums, publication, and rollback.

The [technical specification](../spec.md), [roadmap](../ROADMAP.md), and
[changelog](../CHANGELOG.md) remain the authoritative records for intended
behavior, delivery status, and released changes.
