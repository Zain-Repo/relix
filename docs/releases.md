# Relix release process

The public repository is [Zain-Repo/relix](https://github.com/Zain-Repo/relix).
Stable versions use X.Y.Z in Cargo.toml and vX.Y.Z Git tags.

## Pull requests

Every PR targeting main or master runs when opened, updated, reopened or marked
ready for review, regardless of its author. Checks run on Windows x64, Ubuntu
24.04 x64, macOS 15 Apple Silicon and macOS 15 Intel. Forks use the ordinary
pull_request event and receive no release credentials.

1. Update the package version in Cargo.toml and the matching Relix entry in
   Cargo.lock. Keep dependency versions unchanged.
2. Add releases/vX.Y.Z.md describing implemented changes and known limitations.
3. Open the PR. The version must exceed existing stable tags. Formatting,
   all-target checking, Clippy with warnings denied and tests precede builds.
4. Native packages and checksums appear as workflow artifacts for 14 days.
   Required checks succeeds only when every check and package succeeds.
5. Merge the PR. The sequence runs again on the resulting branch commit. Only
   then does the release job create the matching tag and publish all packages.

Main/master pushes also run this sequence. An unchanged version on a new commit
fails validation. Rerunning an already published commit leaves its release
unchanged. Manual workflow dispatch can retry publication on main/master.

Stages run sequentially: version validation, tests, native release builds,
aggregate required check, publication. Platforms within a stage run in parallel.
Obsolete PR runs are cancelled. Release runs are serialized and never cancelled.
Only the publication job has repository write permissions. Checkout never retains
credentials; action references use full commit SHAs. Dependabot proposes weekly
Actions updates for review.

## Distribution

| Platform | Asset suffix | Contents |
| --- | --- | --- |
| Windows x64 | windows-x64.zip | relix.exe and app-local MSVC CRT DLLs |
| Linux x64 | linux-x64.tar.gz | relix executable |
| macOS Apple Silicon | macos-arm64.tar.gz | Relix.app bundle |
| macOS Intel | macos-x64.tar.gz | Relix.app bundle |

Assets are named Relix-X.Y.Z followed by the platform suffix. All archives include
README, release notes and this process document. SQLite, DuckDB and GPUI assets
are compiled into the app. Windows CRT DLLs come only from Visual Studio's
licensed x64 redistributable directory. macOS builds reject unbundled non-system
dynamic libraries and use ad-hoc bundle signing.

Linux builds check for missing shared libraries on the runner. Packages target
Ubuntu 24.04 or compatible systems with fontconfig, Wayland/X11, xkbcommon,
OpenSSL, Vulkan and D-Bus runtime libraries. A graphical desktop and a working
graphics driver are needed. Remembering passwords requires an unlocked Secret
Service keyring. Packages are not self-contained installers.

Each build executes the staged executable with --version and checks its output
against Cargo.toml. This validates native loading and package identity without
opening a GUI or touching user workspace files. It does not verify rendering,
desktop interaction, live PostgreSQL/MySQL connections or an unlocked OS keyring.
The credentials::tests::native_store_round_trip probe remains ignored by normal
tests because it accesses an actual OS store. Run it only in an isolated desktop
test account. Before broad distribution, manually test the GUI and real server
connections on each supported platform.

Packages are unsigned by a trusted publisher and macOS bundles are not notarized.
There is no automatic updater. Release notes record these limitations.

## Publication and recovery

The publisher requires every archive, verifies its SHA-256 against the build
job's sidecar checksum, and writes SHA256SUMS.txt. It creates a draft at the exact
tested commit, uploads all packages, checks their GitHub-reported sizes and
SHA-256 digests, then publishes. An incomplete upload leaves a draft, which can
be completed by rerunning the same commit. Published tags are never moved.
Draft verification resolves the release ID through the GitHub CLI and reads
the release-by-ID API; the release-by-tag API excludes unpublished drafts.
Fix source defects with a new version and PR. Checksums verify file integrity;
they are not publisher signatures.

To build locally, use Rust 1.96.0 and the native prerequisites documented in
README. Run formatting, Cargo check, Clippy and tests before packaging. For
Windows, run cargo build --release --locked --target x86_64-pc-windows-msvc,
then python scripts/release.py package --target x86_64-pc-windows-msvc.
Choose the native target from scripts/release.py. Packaging requires Python 3.11
or newer. dist/ and target/ are generated output.

Linux prerequisites follow the [pinned GPUI Kit 0.7.1 installer](https://github.com/longbridge/gpui-kit/blob/v0.7.1/script/install-linux.sh).
Permissions follow [GitHub workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax).
Publication uses [GitHub CLI release commands](https://cli.github.com/manual/gh_release_create).
