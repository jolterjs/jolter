# Security Model

Jolter treats release metadata, archives, and archive contents as untrusted
inputs even when they come from an official provider.

## Network

- only `https` URLs are accepted
- redirect targets must remain HTTPS
- metadata responses are bounded to 16 MiB
- artifact responses and extracted archives are bounded to 4 GiB
- requests use finite connection and total timeouts
- transient failures use at most three attempts with bounded backoff

## Integrity

Node.js checksums come from the matching official `SHASUMS256.txt`. Bun and
Deno use GitHub release SHA-256 digests, with provider checksum files as a
fallback. npm, pnpm, and Yarn packages use SHA-512 Subresource Integrity values
from official npm registry metadata. Exact Corepack-style declarations may add
a SHA-1 or SHA-2 archive hash, which Jolter verifies in addition to registry
SRI. Legacy SHA-1 descriptors never replace the required SHA-512 registry
check. Verified archives are cached by integrity identity.

## Extraction

ZIP paths must pass the archive library's enclosed-path check. TAR paths,
hard links, and symbolic links are validated before creation. Extraction
rejects parent traversal, absolute paths, unsupported entries, unsafe parent
symlinks, excessive entries, and excessive expanded size.

## Installation

An installation is built in a temporary directory under the target runtime
directory. It becomes visible only after its expected executable exists and
the completed directory is renamed into place. A per-version file lock
serializes concurrent installers.

Jolter does not run a downloaded executable as part of installation.

Metadata cache writes use per-key locks. Cache cleanup uses an exclusive
maintenance lock, while install, repair, uninstall, and prune participate in
the same locking protocol.

`jolter doctor` may execute an already-installed runtime or package manager
with `--version`. That probe is bounded to five seconds and captures at most
16 KiB from each output stream.

Download progress is calculated locally from response byte counts and elapsed
time. It does not transmit terminal, timing, or usage information and does not
introduce telemetry.
