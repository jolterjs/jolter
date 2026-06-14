# Security Model

Jolter installs and executes developer toolchains, so its primary security goal
is to prevent unverified network or archive content from becoming a published
managed installation.

## Trust Boundaries

Jolter treats these as untrusted input:

- provider metadata and HTTP headers;
- redirects and artifact URLs;
- downloaded archives and package tarballs;
- archive entry paths, links, permissions, and expanded size;
- project configuration and compatibility files;
- environment variables affecting storage, proxies, certificates, and output;
- existing files under a writable `JOLTER_HOME`.

Official provider status does not bypass validation.

The operating system, Rust TLS implementation, Jolter binary, configured trust
store, and authenticated provider integrity metadata remain part of the trusted
computing base.

## Bootstrap Boundary

The one-line installers execute code obtained from:

```text
https://jolter.dev/win/install
https://jolter.dev/unix/install
```

That bootstrap is a separate trust boundary from Jolter's runtime installer.
Security-sensitive environments should download and review the script first or
install a release archive after manually verifying its published SHA-256 file.
See [installation](installation.md).

## Transport

Jolter accepts only HTTPS metadata and artifact URLs. Redirect targets are
validated and must remain HTTPS.

Requests use finite connection and total timeouts. Metadata responses are
bounded to 16 MiB. Artifact downloads and extracted content are bounded to
4 GiB. Retry behavior is limited to three attempts with bounded backoff for
specific transient statuses.

Jolter does not offer an option to disable TLS verification.

## Integrity

Runtime integrity sources:

- Node.js: matching official `SHASUMS256.txt`;
- Bun: GitHub release SHA-256 digest, with provider checksum data where
  available;
- Deno: GitHub release SHA-256 digest, with provider checksum data where
  available.

Managed tool integrity:

- npm registry SHA-512 Subresource Integrity metadata is required;
- exact Corepack-style SHA-1 or SHA-2 hashes are verified as an additional
  constraint;
- a legacy SHA-1 declaration never replaces registry SHA-512 verification.

Verified archives are cached by integrity identity rather than trusted merely
because a filename already exists.

## Archive Extraction

ZIP entries must pass enclosed-path validation. TAR entries, hard links, and
symbolic links are validated before creation.

Extraction rejects:

- absolute paths;
- parent-directory traversal;
- unsupported entry types;
- unsafe parent symlinks;
- links escaping the staging root;
- excessive entry counts;
- excessive expanded size.

Extraction occurs in a staging directory, not directly over a live
installation.

## Atomic Publication

A runtime or tool becomes visible at its final path only after:

1. artifact integrity succeeds;
2. extraction succeeds;
3. the required executable or entry point exists;
4. the installation manifest is written;
5. the staging directory is atomically renamed.

Jolter does not execute a downloaded runtime or tool as part of installation.

Exact-version file locks serialize concurrent publication and removal.
Metadata-key locks serialize cache publication. An exclusive maintenance lock
prevents cache cleaning from racing operations that use cached artifacts.

## Filesystem Permissions

`JOLTER_HOME` should be writable only by the user or trusted automation
identity that owns it. If another principal can modify managed executables,
tool entry points, manifests, shims, or active state, that principal can affect
future command execution.

Do not share one writable home between mutually untrusted CI jobs,
repositories, or users.

Project configuration is code-adjacent input. Review changes to `jolter.json`,
`.node-version`, `.nvmrc`, and `package.json#packageManager` with the same care
as build configuration.

## Execution

Normal shims execute the runtime or tool selected by project resolution.
Managed JavaScript tools run through the selected Node.js executable.

`jolter doctor` may execute already-installed runtimes and tools with
`--version`. Each probe:

- is limited to five seconds;
- captures at most 16 KiB per output stream;
- receives `JOLTER_DOCTOR=1`.

Do not run doctor against an untrusted restored home without first treating
the installed executables as potentially hostile.

## Proxies and Certificates

Proxy and custom-CA configuration can redirect or alter network trust. Protect
proxy credentials and CA files. Prefer CI secret injection over credentials in
plain environment values, and never work around a TLS issue by disabling
verification.

See [networking and offline mode](networking-and-offline.md).

## Cache Security

Cache restore sources must match the intended operating system, architecture,
and trust boundary. Jolter re-verifies artifact integrity before publication,
but malicious modification of writable metadata, lock state, or already
installed executables remains a local filesystem concern.

Use immutable or scoped cache keys for untrusted contribution workflows.

## Telemetry and Privacy

Jolter has no usage telemetry. Download progress is calculated locally from
response byte counts and elapsed time.

Commands can print local paths, provider names, versions, and diagnostic
environment findings. Review JSON reports and logs before attaching them to a
public issue.

## Security Non-Goals

Jolter cannot protect against:

- a compromised operating system or Jolter binary;
- an attacker with write access to the user's executable paths or
  `JOLTER_HOME`;
- malicious code intentionally published by a trusted provider under valid
  integrity metadata;
- arbitrary code run by a selected runtime, package script, project, or tool;
- secrets exposed by the surrounding shell, proxy, CI configuration, or logs.

## Reporting

Do not publish a suspected vulnerability with working exploit details before
maintainers have had an opportunity to respond. Include:

- affected Jolter version and platform;
- minimal reproduction;
- trust-boundary assumptions;
- observed and expected behavior;
- whether untrusted network, archive, project, or local filesystem input is
  required.

Use the repository's private security-reporting channel when available.
