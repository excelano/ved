# Releasing ved

The release loop lives in `~/notes/releasing.md` — the ordered steps, the apt
step, crates.io, the winget submission, the spent-tag rule, and the standing
facts about tokens and secrets. Failure recipes are in
`~/notes/build_release_gotchas.md`. This file carries what is true of ved and not
of its siblings.

| | |
|---|---|
| Loop | cargo-dist |
| Version lives in | `version` in `Cargo.toml` |
| `apt-ship` argument | `ved` |
| crate | `ved` |
| winget package | `Excelano.ved` |
| Windows asset | `ved-x86_64-pc-windows-msvc.zip` |

**The release builds** the five platform tarballs, the shell and PowerShell
installers, the Homebrew formula, and the checksums, then creates the GitHub
Release. The `.deb` packages come from the separately dispatched `deb.yml`, and
cargo-deb names them with a package revision — `ved_1.2.3-1_amd64.deb`.

**ved is a clone of `ed`, so compatibility is the release risk.** The tests cover
the command grammar, but the thing that breaks quietly is a real `ed` script
behaving differently under ved. Running one through the new build before tagging
is cheap insurance for the claim the README makes.

**ved trips `Validation-Executable-Error` by design.** Bare invocation with no
arguments takes an intentional usage guard and exits non-zero, which winget's
bare-invocation sweep reports as a failure. Recipe in the gotchas file; do not
change the guard to appease it.
