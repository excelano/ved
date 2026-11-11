# Security Policy

## Reporting a vulnerability

Please report suspected vulnerabilities privately through GitHub Security Advisories at https://github.com/excelano/ved/security/advisories/new. If you would rather not use GitHub, email david.anderson@excelano.com instead. I aim to respond within seven days.

Please do not open public issues for security problems.

## Supported versions

The latest release receives security fixes. Older versions are not supported. A fix ships in a new release; update the way you installed.

## What ved can access

ved is a CLI line editor that runs locally on your machine. It reads the file you point it at, holds it in memory for the duration of the session, and writes the buffer back to disk only when you issue a write command. The `edit` and `read` commands open additional files at your request. ved makes no network calls of any kind, has no auth layer, and does not implement administrative operations. It can only read and write files your operating-system user already has access to.

## What ved stores

ved stores nothing outside the files you explicitly write. No history file, no config directory, no telemetry, no analytics, no remote logging.

## Verifying releases

Every GitHub release carries the `.deb` packages and a Windows zip, built by `build-release`, the release program in excelano/shipping, from a tagged commit. The Excelano apt repository is signed, so `apt` verifies each package it installs, and WinGet checks the zip against the SHA-256 recorded in its manifest. To check a file downloaded from the release page by hand, compare its `sha256sum` with the digest GitHub lists for that asset.
