# Diagnostics and Inventory

## Installed Toolchains

Run:

```bash
jolter list
```

The command lists both runtime and managed package manager directories.

- `*` marks the globally active exact runtime.
- `ready` means the expected executable or package manager entry point exists.
- `incomplete` means a version directory exists but its expected entry point
  is missing.

Incomplete project requirements can be restored with:

```bash
jolter repair
```

## Health Checks

Run:

```bash
jolter doctor
```

Checks currently cover:

- storage location;
- project requirement resolution;
- runtime availability;
- managed package manager availability;
- installed shims;
- shims directory presence on `PATH`.

Warnings do not make the command fail. A failed check produces a nonzero exit
status.

## JSON Output

Automation can use:

```bash
jolter doctor --json
```

Example shape:

```json
{
  "checks": [
    {
      "message": "using /home/user/.jolter",
      "name": "storage",
      "status": "pass"
    }
  ],
  "healthy": true
}
```

Check status values are:

- `pass`
- `warning`
- `fail`

The JSON command uses the same exit status rules and checks as the
human-readable command.
