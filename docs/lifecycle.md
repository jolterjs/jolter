# Storage Lifecycle

## Uninstall

Uninstall requires an exact semantic version:

```bash
jolter uninstall node@24.1.0
jolter uninstall pnpm@10.2.0
```

An active runtime or package manager is protected. Activate another version
first, or explicitly remove and deactivate it:

```bash
jolter uninstall node@24.1.0 --force
jolter uninstall pnpm@10.2.0 --force
```

## Prune

Preview before removing anything:

```bash
jolter prune --dry-run
```

By default, prune keeps the newest complete version of each runtime and
package manager. Active versions and versions required by the current project
are always protected.

```bash
jolter prune --keep 2
```

## Cache

```bash
jolter cache status
jolter cache clean
```

Cache cleaning removes downloaded archives and release metadata, but not
installed toolchains or lock files. Offline sync may fail after a cache clean
until the required metadata and archives are downloaded again.
