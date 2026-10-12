# Application updates

Cash Ledger's installed Windows x64 app checks the latest stable release of
`Zain-Repo/cash-counter` when it starts. Settings shows the current version,
update status, automatic-update preference, and manual check/install controls.
Automatic updates are enabled initially. Development copies and GPUI tests do
not automatically check for or install releases.

## Download, install, and restart

The updater accepts only a strictly newer stable version with an exact
`CashLedger-<version>-windows-x64-setup.exe` asset. It requires the SHA-256 digest
and asset size supplied by the GitHub Releases API. Metadata and downloads have
size limits and HTTP timeouts. Download redirects must stay on the allowed
GitHub HTTPS hosts. A mismatched digest, missing installer, network error, or
invalid release leaves the current application installed.

Downloads run independently of ledger storage tasks. A verified update waits
while there is a save in progress, a failed cash save awaiting retry, or an
unfinished form. Saving or clearing the draft allows installation to proceed;
Settings also provides a manual install-and-restart control. No financial draft
is discarded to restart the app.

The app stages a separate helper executable and its app-local runtime. The
helper validates the downloaded installer and opens a handle to the running
application before acknowledging readiness. Only then does the GUI exit. The
helper waits for that process to terminate and invokes the existing per-user
Inno Setup installer, targeting the same installation directory.
The helper converts canonical Windows paths to ordinary DOS/UNC paths for
Inno Setup's command-line arguments. Each attempt writes `setup.log` in its
temporary `cash-ledger-update-*` directory; installation failures display the
log location for diagnosis.

The installer never force-closes another app instance. Its application mutex
prevents replacement while another instance is running. Installation failures
are recorded for the next launch; automatic installation pauses after failure
so a restored application cannot enter a restart loop. A reboot-required exit
does not relaunch a partially replaced app. After successful installation, the
installer starts the new version once. Sign in again to access the workspace.

The public display name is Cash Ledger. The installer AppId, legacy
`%LOCALAPPDATA%/Programs/Till` directory, `cash-counter.exe` executable name, and
existing Cash Register database location remain stable. Neither updates nor
uninstall remove the ledger or local company credentials.

## Publishing updates

Follow the [release process](releases.md): increase `Cargo.toml`'s version, add
release notes, pass the checks, and publish a matching stable `vX.Y.Z` tag. CI
builds and publishes the installer. GitHub supplies the release-asset digest
used by the updater; the accompanying `SHA256SUMS.txt` supports manual checks.
Older Till releases need one manual installation of an updater-enabled release
before they can update themselves. This code change does not publish a release.

The updater trusts this GitHub repository and HTTPS transport. A SHA-256 digest
checks the asset against trusted release metadata; it is not an independent
publisher signature. Protect repository release permissions. Installers are
currently unsigned; Authenticode signing is a separate release-infrastructure
step, not something the updater claims to verify.

Before releasing, use an isolated database and installed Windows VM to verify
upgrade, relaunch, multiple running instances, failed downloads, unsaved drafts,
and installer failure/reboot behavior. Automated tests do not run a downloaded
installer or update this development machine.

## Research references

- [GitHub release assets](https://docs.github.com/en/rest/releases/assets)
  documents the `digest`, size, and download URL fields.
- [GitHub asset digests](https://github.blog/changelog/2025-06-03-releases-now-expose-digests-for-release-assets/)
  explains GitHub's automatic SHA-256 calculation.
- [Inno Setup command-line parameters](https://jrsoftware.org/ishelp/topic_setupcmdline.htm)
  documents silent installation and reboot exit codes.
- [Inno Setup AppMutex](https://jrsoftware.org/ishelp/topic_setup_appmutex.htm)
  documents preventing replacement while an application is running.
- [Inno Setup launch entries](https://jrsoftware.org/ishelp/topic_runsection.htm)
  documents `postinstall`, `runasoriginaluser`, and silent-launch behavior.
- [Rust self_update](https://docs.rs/self_update/1.3.0/self_update/)
  supports standalone executable updates. Cash Ledger instead keeps its existing
  installer responsible for runtime files, shortcuts, and uninstaller metadata.
