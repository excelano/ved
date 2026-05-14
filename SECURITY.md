# Security Policy

## Reporting a vulnerability

Please report suspected vulnerabilities privately through GitHub Security Advisories at https://github.com/excelano/ved/security/advisories/new. If you would rather not use GitHub, email david.anderson@excelano.com instead. I aim to respond within seven days.

Please do not open public issues for security problems.

## Supported versions

The latest 0.x release receives security fixes. Older versions are not supported.

## What ved can access

ved is a CLI line editor that runs locally on your machine. It reads the file you point it at, holds it in memory for the duration of the session, and writes the buffer back to disk only when you issue a write command. The `edit` and `read` commands open additional files at your request. ved makes no network calls of any kind, has no auth layer, and does not implement administrative operations. It can only read and write files your operating-system user already has access to.

## What ved stores

ved stores nothing outside the files you explicitly write. No history file, no config directory, no telemetry, no analytics, no remote logging.

## Verifying releases

Every GitHub release includes a `.sha256` file next to each archive listing its SHA-256 hash. Verify any download before running it:

    sha256sum ved-x86_64-unknown-linux-gnu.tar.xz
    # compare against the value in ved-x86_64-unknown-linux-gnu.tar.xz.sha256

Release artifacts are built by GitHub Actions from a tagged commit using the cargo-dist configuration in this repo (`dist-workspace.toml` and the generated `.github/workflows/release.yml`). The workflow and build configuration are public and auditable.
