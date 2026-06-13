# Changelog

## 0.2.0 - 2026-06-13

- add the `[jolter]` terminal output style with live download bytes, percentage,
  transfer rate, and current installation actions
- fix literal ANSI escape codes in legacy Windows consoles and align `list`
  status and path columns using calculated widths
- add global `--no-progress`, `--no-color`, `--quiet`, and `--verbose` output
  controls with automatic CI and non-terminal fallback
- allow `jolter use` to install and activate npm, pnpm, and Yarn versions
- add exact-version uninstall, project-aware prune, and cache cleanup
- add bounded runtime and package-manager version probing
- add manifest, permission, engine, cache, proxy, and PATH precedence
  diagnostics with remediation
- add `list --json`, `setup-ci`, and shell completion generation
- add configuration schema version 1 compatibility rules
- add bounded provider retries and per-metadata-key concurrency locks
- add native cross-platform, WSL, coverage, audit, provider-smoke, and release
  workflows
- raise measured line coverage above the 80% project target
