# Updating Relix

Relix 0.1.2 introduces an updater for the portable packages published at
[Zain-Repo/relix](https://github.com/Zain-Repo/relix/releases). Release publication
and installing an update are separate operations: CI publishes a release only
after all four platform builds pass; the running application checks for that
published release and asks before restarting to install it.

Open **Updates** in the toolbar to see the installed version and update status.
Automatic updates are enabled by default: Relix checks at startup and every six
hours and downloads a verified newer package in the background. Choose **Restart
to update** to open the dialog, then **Restart and install** when ready.
Disabling automatic updates leaves **Check for updates** and
**Download update** available for manual use. Downloads are temporary and session-only;
closing Relix before installation requires a new download in the next session.

## Upgrading 0.1.0 or 0.1.1

These versions have no updater and cannot acquire one without a manual upgrade.

1. Save query drafts and finish or roll back open transactions. Close Relix.
2. Download the latest matching Windows x64, Linux x64, macOS Apple Silicon or
   macOS Intel package from the official release page. Verify its SHA-256 against
   the release's `SHA256SUMS.txt` when downloading manually.
3. Extract the whole `Relix` directory into a writable installation location.
   On Windows retain the supplied CRT DLLs with `relix.exe`; on macOS keep the
   complete `Relix.app` bundle. Do not mix executables and support files from
   different versions. Keep the previous package until the new app starts.
4. Launch the new executable or bundle and update any shortcut pointing to the
   old location. Subsequent supported releases can use the in-app updater.

Application files are separate from the existing workspace and OS credential
store. Replacing a package does not move database files, clear connection
profiles, saved queries or history, or remove remembered passwords. Keep the
same OS account. Unsaved drafts are session-only and must be saved before exit.
Workspace location and credential behavior are described in [studio.md](studio.md).

## Installation safety

Restart is blocked while database connections remain open, operations are
running, or query drafts need saving. Finish operations, explicitly commit or
roll back transactions, disconnect every database, and save or close drafts
before accepting the restart. Downloading an
update does not authorize database writes or execute SQL.

Use the official portable package with a writable installation and parent
directory. Development builds,
unsupported architectures, and managed or protected installations may require
manual replacement. No elevated installer is used. Networking, missing digest,
corrupt download or installation failures must leave a usable prior installation
and expose a retry or manual download path.

The update helper runs separately after exit so it can replace locked Windows
files. It stages files next to the installation in a `.relix-update-*` directory,
rechecks file digests and keeps per-file backups. If copying or the new executable's
version check fails, it restores replaced files. Recovery files and `result.log`
remain in that directory after success or failure; inspect them if an update fails
after exit. Successful recovery directories are not automatically removed. After
confirming the updated app works, they can be removed manually to reclaim space.
If automatic
relaunch fails, reopen Relix manually. Archive extraction rejects links, special files and
paths escaping the package directory. Compressed packages are limited to
512 MiB and expanded contents to 1 GiB. Links and Windows junctions inside the
installation are rejected; use a real, writable installation directory.

## Release trust

The updater consumes the fixed repository's GitHub Releases API over HTTPS,
selects the exact platform asset name, and verifies downloaded bytes against its
GitHub-reported `sha256:` digest. Draft and prerelease releases are excluded;
only a newer stable version is eligible. The release publisher verifies every
archive's build checksum, uploaded size, GitHub digest and official download URL
before making the draft public. No custom update feed or extra secret is needed.

This trusts GitHub, HTTPS and the repository's release permissions. A checksum
does **not** provide an independent publisher signature: compromise of the
release account or its publishing workflow can replace both package and digest.
Packages remain unsigned by a trusted publisher; macOS bundles have ad-hoc
signing and are not notarized. Adding independent signing requires a separate
key-management and recovery plan.

## Implementation validation

Windows validation for 0.1.2 passed formatting, Cargo check, Clippy with warnings
denied, the native build, 106 Rust tests (one existing credential test ignored),
and 17 release-publisher tests. Native helper smoke tests used disposable copies
of the built executable with distinct old/new file digests: successful replacement,
backup preservation, process exit handoff, version probing and GPUI relaunch were
observed. A deliberately mismatched version restored the original bytes and
reopened the previous installation. No installed user application was replaced.

This validates the helper protocol, not a published older-to-newer release pair.
Native Linux and macOS update/restart testing remains outstanding. The release
pipeline builds and checks packages for all four supported targets.

## DBX research and evidence limits

The implementation approach was informed by DBX's source rather than copying its
Tauri integration. Reviewed primary sources:

- [DBX updater composable](https://github.com/t8y2/dbx/blob/main/apps/desktop/src/composables/useAppUpdater.ts)
  checks on startup and hourly, stages downloads, and asks a preparation callback
  to freeze work before installation. It tracks checking, downloading, ready and
  installation states and provides bounded retries and cancellation.
- [DBX native update commands](https://github.com/t8y2/dbx/blob/main/src-tauri/src/commands/update.rs)
  reverify cached bytes immediately before installation, bind installation to the
  expected version, and distinguish installers from portable helper handoff.
- [DBX Tauri configuration](https://github.com/t8y2/dbx/blob/main/src-tauri/tauri.conf.json)
  enables updater artifacts and embeds a public verification key with official
  and GitHub update-feed endpoints. Relix's current GitHub digest verification
  does not offer the same independent signing boundary.

These are source observations checked against the online repository on
2026-10-12 and the local `dbx-reference` graph. They do not prove a specific
installed build executed these paths. Current online configuration identifies
DBX 0.6.41; Windows version resources identify the installed DBX as 0.6.26.

The requested reverse-engineering skill was applied to
`C:/Program Files/DBX/dbx.exe`. REA identified an x64 PE with SHA-256
`3d2f23626335dd237f57310c51e3d3dd8b742d5d818976f63987397d550d6edc`.
Its execution-free PE resource inventory examined 22 entries and 9 resources
(Evidence `ev_61a8e862f8c9c8359791a764089e70e0162b050fe06612676a53d93bfca146b9`).
Native updater strings and decompilation could not be examined: Ghidra and IDA
were unconfigured and Hopper does not support this Windows host. No DBX process
was launched, no update installed, and no analysis engine or machine settings
were changed. Installed updater behavior remains unverified.
