# Storage and Lifecycle

Jolter owns a self-contained storage root for runtimes, tools, shims, cache,
locks, and global activation state.

## Storage Root

Default:

```text
Windows: %USERPROFILE%\.jolter
Unix:   $HOME/.jolter
```

Override:

```text
JOLTER_HOME
```

Layout:

```text
.jolter/
|-- runtimes/
|   |-- node/
|   |-- bun/
|   `-- deno/
|-- tools/
|   |-- npm/
|   |-- pnpm/
|   `-- yarn/
|-- shims/
|-- cache/
|   |-- downloads/
|   |-- metadata/
|   `-- locks/
`-- config/
    `-- active.json
```

Version directories are named with exact semantic versions. Runtime
installations contain `.jolter-install.json`; managed tools contain
`.jolter-tool.json`.

These files are implementation state. Do not edit them manually.

## Operation Boundaries

Publication or removal of one exact version is lock-coordinated and atomic at
the final filesystem boundary. Commands that operate on several versions, such
as `sync`, `update --all`, and `prune`, are not one global transaction.

If a later target fails, earlier successful work remains. These commands are
designed to be rerun: complete compatible installations are reused, and
remaining work continues.

## Atomic Publication

Downloads and extraction occur in temporary locations. A version becomes
visible at its final path only after integrity verification and required
payload checks succeed. Exact-version locks prevent concurrent processes from
publishing the same installation simultaneously.

An incomplete version directory can still exist after external modification,
manual copying, or an older interrupted operation. `jolter list` exposes it
and `jolter repair` can restore a project requirement.

## Uninstall

```bash
jolter uninstall node@24.5.0
jolter uninstall pnpm@10.12.1

# you can also use "jolter rm" or "jolter remove"
```

The version must be exact. Jolter reports the removed path and reclaimed size.

Active versions are protected:

```bash
jolter uninstall node@24.5.0 --force
```

`--force` removes the version and clears its global activation. It does not
edit project files, so a future `sync` can install it again.

Before force removal:

```bash
jolter list
jolter doctor
```

## Prune

Preview:

```bash
jolter prune --dry-run
```

Apply default retention:

```bash
jolter prune
```

Keep two newest complete versions of each kind:

```bash
jolter prune --keep 2
```

Prune protects:

- every globally active exact version;
- matching installed versions required by the current project;
- the newest requested count of complete versions for every runtime/tool kind.

Incomplete installations are not part of the complete-version retention count
and are removable unless otherwise protected.

Run prune from the project whose requirements must be protected. In a
multi-project machine, project-specific versions required only by another
directory are not automatically discovered.

## Cache

Inspect:

```bash
jolter cache status
```

Clean:

```bash
jolter cache clean
```

Cache cleaning removes downloaded artifacts and provider metadata. It does not
remove:

- installed runtimes or tools;
- shims;
- project configuration;
- global active selections;
- lock coordination files.

Cleaning can make the next synchronization slower and can break offline
operation until required data is downloaded again.

## Locking

Jolter uses:

- exact runtime-version locks;
- exact tool-version locks;
- metadata-key locks;
- a shared/exclusive maintenance lock.

Install and repair participate in maintenance coordination so cache cleaning
cannot remove an artifact while it is in use. Uninstall and prune use the same
version-lock protocol as publication.

Do not delete lock files while Jolter processes are active.

## Backups

The project declarations that matter for reproducibility belong in source
control. `JOLTER_HOME` is rebuildable state.

For workstation backup:

1. stop Jolter operations;
2. preserve `config/active.json` only if global defaults matter;
3. preserve the cache only if offline recovery matters;
4. treat runtime/tool directories as platform-specific executables;
5. restore only onto a compatible operating system and architecture.

After restoration:

```bash
jolter setup
jolter doctor
```

## Moving `JOLTER_HOME`

Set the new path consistently, then:

```bash
jolter setup
jolter doctor
```

Copying existing installations is not guaranteed across platforms or
architectures. The safest migration is to retain project configuration and run
`jolter sync` into the new home.

## Manual Cleanup

Prefer `uninstall`, `prune`, and `cache clean`. Manual deletion bypasses
activation cleanup, locks, reclaimed-size reporting, and compatibility checks.

If manual recovery is unavoidable, stop all Jolter processes, back up
`JOLTER_HOME`, make the smallest targeted change, then run:

```bash
jolter list
jolter setup
jolter doctor
```
