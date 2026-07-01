# Troubleshooting

Start every investigation from the affected directory and shell:

```bash
jolter doctor
jolter list
```

Use `--no-progress` when collecting logs and `--json --no-color` when a
machine-readable doctor report is needed.

## `jolter` Is Not Found

Locate the executable:

PowerShell:

```powershell
Get-Command jolter -All
```

Unix:

```bash
command -v jolter
type -a jolter
```

If the binary exists, invoke it by absolute path and run:

```bash
jolter setup
```

Apply the printed `PATH` command and restart the entire terminal or IDE.

## A Command Resolves Outside Jolter

Symptoms include the wrong Node.js version or `Get-Command`/`type -a` showing
nvm, fnm, Volta, Corepack, or a system installation first.

Run:

```bash
jolter setup
jolter doctor
```

Move `$JOLTER_HOME/shims` before competing entries on `PATH`. Restart GUI
applications that inherited an older environment.

## The Project Uses the Wrong Version

Confirm the current directory and inspect requirement files:

```bash
jolter doctor
```

Review precedence in [project resolution](project-resolution.md). Important
cases:

- `jolter.json` runtime wins over `.node-version` and `.nvmrc`;
- `.node-version` wins over `.nvmrc`;
- a non-empty `jolter.json#tools` map wins over
  `package.json#packageManager`;
- project requirements win over globally active versions.

After correcting the declaration:

```bash
jolter sync
```

## Required Version Is Missing

```bash
jolter sync
```

Shims do not install during command dispatch. A project must be synchronized
before its declared commands can run.

If the directory exists but inventory says `incomplete`:

```bash
jolter repair
```

## A Managed Tool Requires Node.js

Managed npm, pnpm, and Yarn versions execute through Node.js.

For a global tool:

```bash
jolter use node@lts
jolter use pnpm@10
```

For a project, configure Node.js as the runtime:

```bash
jolter pin node@24
jolter pin pnpm@10
jolter sync
```

A project using Bun or Deno as its sole runtime cannot synchronize Jolter
managed tools.

## Tool and Node.js Are Incompatible

Jolter validates `engines.node`. Choose a compatible pairing:

```bash
jolter use node@24
jolter use pnpm@10
```

For a project, update either selector, then run `jolter sync`. Do not bypass
the check by editing an installed package's metadata; repair and doctor treat
managed installation data as authoritative.

## Update Says No Active Version Exists

A bare update needs a global active version to derive its major line:

```bash
jolter update node
```

Provide an explicit selector instead:

```bash
jolter update node@latest
```

Or activate a version first with `jolter use`.

## Offline Cache Miss

Check whether offline mode is enabled:

PowerShell:

```powershell
$env:JOLTER_OFFLINE
```

Unix:

```bash
printf '%s\n' "$JOLTER_OFFLINE"
```

Disable it, synchronize once with network access, and then re-enable it:

```bash
jolter sync
jolter cache status
```

`jolter cache clean` deliberately removes offline readiness.

## Proxy or Certificate Failure

Inspect:

```text
HTTPS_PROXY
HTTP_PROXY
NO_PROXY
SSL_CERT_FILE
NODE_EXTRA_CA_CERTS
REQUESTS_CA_BUNDLE
```

Proxy URLs must use `http://` or `https://`. Certificate variables must point
to existing files. Run `jolter doctor` in the same environment as the failed
installation.

See [networking and offline mode](networking-and-offline.md).

## Permission or Read-Only Storage Failure

Confirm the resolved root in `jolter doctor`. Select a writable user path:

PowerShell:

```powershell
$env:JOLTER_HOME = "$HOME\.jolter"
jolter doctor
```

Unix:

```bash
export JOLTER_HOME="$HOME/.jolter"
jolter doctor
```

Do not run normal Jolter operations as an elevated account merely to work
around ownership created by a previous elevated run. Correct ownership and
permissions according to the operating system.

## Active Version Cannot Be Removed

Activate another version:

```bash
jolter use node@22
jolter uninstall node@24.5.0
```

Or intentionally remove and deactivate it:

```bash
jolter uninstall node@24.5.0 --force
```

Project declarations are not changed by uninstall.

## JSON Output Is Corrupted

Do not merge standard error into standard output. Use:

```bash
jolter doctor --json --no-color > doctor.json
```

Progress and failures belong to standard error. JSON belongs to standard
output.

## Colors Appear as Escape Codes

Use:

```bash
jolter list --no-color
```

Or set `NO_COLOR=1` or `JOLTER_NO_COLOR=1`. Jolter automatically disables ANSI
styling when the terminal does not advertise support, but wrappers can
misreport terminal capabilities.

## Progress Is Noisy in Logs

```bash
jolter sync --no-progress
```

This keeps useful line-oriented action events. Use `--quiet` to suppress them
and keep only final results and failures.

## Repair Does Not Fix the Problem

Collect:

```bash
jolter list --json
jolter doctor --json --no-color
jolter cache status
jolter repair --verbose --no-progress
```

Preserve the exact Jolter version, operating system, architecture, project
requirements, and error text. Avoid publishing credentials, proxy secrets, or
private filesystem details in a public report.
