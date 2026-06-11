# Security Model

Jolter treats release metadata, archives, and archive contents as untrusted
inputs even when they come from an official provider.

## Network

- only `https` URLs are accepted
- redirect targets must remain HTTPS
- metadata responses are bounded to 16 MiB
- artifact responses and extracted archives are bounded to 4 GiB
- requests use finite connection and total timeouts

## Integrity

Node.js checksums come from the matching official `SHASUMS256.txt`. Bun and
Deno use GitHub release SHA-256 digests, with provider checksum files as a
fallback. Verified archives are cached by checksum.

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
