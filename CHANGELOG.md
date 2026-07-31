# Changelog

## 0.4.0 - TBD

### What changed?

- [ ]

## 0.3.0 - 2026-08-01

### What changed?

- add schema-version-2 plugin declarations in `jolter.json`
- add global plugin install, list, update, and uninstall commands
- add registry-backed plugin release download and WASM integrity checks
- add dynamic shim generation for commands declared by installed plugins
- add `@jolter/jdk` with the `jdt` plugin packaging CLI at https://github.com/jolterjs/jdt

## 0.2.0 - 2026-06-13

### What changed?

- add the `[jolter]` terminal output style with live download bytes, percentage,
  transfer rate, and current installation actions
- fix literal ANSI escape codes in legacy Windows consoles and align `list`
  status and path columns using calculated widths
- add global `--no-progress`, `--no-color`, `--quiet`, and `--verbose` output
  controls with automatic CI and non-terminal fallback
- allow `jolter use` to install and activate npm, pnpm, and Yarn versions
- add `jolter update` for one active runtime/tool or all active versions
- allow `jolter pin` to preserve and pin multiple tools through the canonical
  `tools` configuration map
- replace package-manager terminology and JSON fields with general tools while
  retaining legacy `packageManager` input compatibility
- add exact-version uninstall, project-aware prune, and cache cleanup
- add bounded runtime and tool version probing
- add manifest, permission, engine, cache, proxy, and PATH precedence
  diagnostics with remediation
- add `list --json`, `setup-ci`, and shell completion generation
- add configuration schema version 1 compatibility rules
- add bounded provider retries and per-metadata-key concurrency locks
- add native cross-platform, WSL, coverage, audit, provider-smoke, and release
  workflows
- raise measured line coverage above the 80% project target
- redesign the README around product onboarding and official Windows and Unix
  installer commands
- expand the documentation into production-ready user, automation, security,
  maintenance, architecture, development, and release runbooks
