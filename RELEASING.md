# Releasing ved

The release loop for a new version. Run it from a clean `main` with the working tree committed. Examples below cut `v1.2.3`; substitute the version you are actually releasing.

1. **Bump the version.** Edit `version` in `Cargo.toml`. Update `Cargo.lock` with a build (`cargo build`), run `cargo test`, commit.

2. **Tag and push.** `git tag v1.2.3 && git push origin main --tags`. The `v*` tag triggers cargo-dist (`.github/workflows/release.yml`), which builds the five platform tarballs, the shell and PowerShell installers, the Homebrew formula, and the checksums, then creates the GitHub Release.

3. **Build the .debs.** cargo-dist creates the release with the default `GITHUB_TOKEN`, and GitHub does **not** fire `release: published` for token-created releases (a documented anti-recursion safeguard). So `deb.yml` won't auto-run — dispatch it by hand:
   ```sh
   gh workflow run deb.yml -f tag=v1.2.3
   ```
   It builds amd64 + arm64 packages and uploads them to the release.

   **This is the step that gets skipped.** Nothing downstream complains: the release page looks finished, the installers work, Homebrew and crates.io are already live, and the only symptom is that apt keeps serving the previous version indefinitely. Before moving on, confirm the two `.deb` files are actually attached to the release.

4. **crates.io publishes itself.** The `v*` tag also triggers `publish-crate.yml`, which runs `cargo publish` with the org-secret token, so crates.io is live within a minute of the push and there is no local step. Confirm it succeeded:
   ```sh
   gh run list --workflow=publish-crate.yml --limit 1
   ```
   Do **not** run `cargo publish` by hand — the pipeline beats you to it and you'll just get `already exists`. **Versions are immutable**: you can `cargo yank` a bad release to hide it from new dependency resolution, but never re-publish the same number. A fix is always a fresh version bump, never a re-push.

5. **Add the .debs to the Excelano apt repo.** Download the two `.deb`s from the release — cargo-deb names them with a package revision, `ved_1.2.3-1_amd64.deb` — then in `~/excelano-apt/`: `add-deb.sh` each one → `rebuild.sh` (GPG-signs) → `updatesite excelano.com.apt -y`. **Dry-run the rsync first** (`rsync … --delete -n`) and confirm zero deletions before the real push — the apt pool is a superset of live, and a stray `--delete` wipe is the standing hazard. See `feedback_rsync_parent_wipes_subpath`.

   `updatesite` does not touch git, so commit the apt repo right afterwards or it drifts behind what is actually being served.

6. **Submit the winget manifest.** winget stores one manifest per version, so every release needs its own PR; there is no update in place. Sync the fork, then run komac:
   ```sh
   komac sync
   komac update Excelano.ved --version 1.2.3 \
     --urls https://github.com/excelano/ved/releases/download/v1.2.3/ved-x86_64-pc-windows-msvc.zip \
     --submit
   ```
   It downloads the asset, computes the `InstallerSha256`, generates the manifest from the previous version's, and opens the PR against `microsoft/winget-pkgs`. Drop `--submit` (or add `--dry-run --output ./dir`) to eyeball the manifest first, and check the generated hash against the release's published `.sha256`.

   **Sync the fork before submitting**, every time. A fork that has drifted behind upstream fails in a way that reads like a permissions problem rather than a stale fork; recipe in `~/notes/build_release_gotchas.md`.

   A **version update** to an already-merged package clears automated validation and merges with no human involved, usually inside a day. A **new package** picks up the `New-Package` label and waits on a volunteer moderator, which runs to days or weeks. Two failures recur, both with recipes in `~/notes/build_release_gotchas.md`: `Validation-Defender-Error` (a Defender heuristic flags the unsigned cargo-dist binary — submit the false positive, never rebuild to appease it) and `Validation-Executable-Error` (validation runs the exe with no arguments and reports a non-zero exit, which an intentional usage guard will trip).

   **A pushed `v*` tag is spent.** The merged manifest pins `InstallerSha256`, so deleting and re-cutting a tag swaps the release asset out from under it and breaks every install of that version. That is the same immutability rule step 4 states for crates.io, except nothing here refuses the second attempt — winget, apt, and the Homebrew formula all overwrite silently. If a release goes wrong after the tag is pushed, bump to the next number.

## Notes

- **ved is a clone of `ed`, so compatibility is the release risk.** The tests cover the command grammar, but the thing that breaks quietly is a real `ed` script behaving differently under ved. Running one through the new build before tagging is cheap insurance for the claim the README makes.
- **crates.io API needs a User-Agent.** Requests without one return empty (`name: None`). To verify a publish from a script: `curl -s -H "User-Agent: …" https://crates.io/api/v1/crates/ved`.
- **First-time crates.io setup:** the `CRATES_IO_TOKEN` org secret must be present for `publish-crate.yml` to fire; a verified crates.io email is required before the first publish; the token needs the `publish-new` + `publish-update` scopes.
- **Homebrew tap access is an org-secret question.** cargo-dist pushes the formula to `excelano/homebrew-tap` with `HOMEBREW_TAP_TOKEN`. If that secret is scoped to selected repositories, a repo that is not on the list fails the `publish-homebrew-formula` job at checkout with `Input required and not supplied: token` while the rest of the release succeeds. Don't re-run that job against the old tag afterwards; push the formula by hand for that release and let the token fix take effect on the next one.
- **docs.rs** rebuilds automatically on each publish — no action needed.
- The README, the landing page (`excelano.com/ved`), and `SECURITY.md` reference the version implicitly via "latest"; none need a per-release edit.
