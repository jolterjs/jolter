# Security Policy

Jolter is pre-release software and has not received an independent security
audit.

Please report vulnerabilities privately through GitHub security advisories
for this repository. Do not open a public issue for an undisclosed
vulnerability.

## Current Guarantees

- release metadata and artifacts are fetched over HTTPS
- redirects to non-HTTPS URLs are rejected
- archives are verified against official SHA-256 metadata before extraction
- archive paths, links, entry counts, and extracted sizes are constrained
- installations are staged and then published atomically
- Jolter does not execute downloaded runtimes during installation
- telemetry is not implemented and no usage data is collected

Downloaded runtimes execute only when the user invokes their Jolter shim.
