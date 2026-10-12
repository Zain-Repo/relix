# Windows packaging

Cash Ledger uses an embedded multi-resolution application icon and an Inno Setup
per-user installer. Packaging targets Windows 10 version 1703 or later and
Windows 11 x64 with the MSVC toolchain.
The Rust executable remains `cash-counter.exe`; the displayed product is Cash Ledger.

## Build

Install Rust's `x86_64-pc-windows-msvc` target, Visual Studio C++ Build Tools
and the Windows SDK as described in [setup](gpui/setup.md). Install
[Inno Setup 6.5 or later](https://jrsoftware.org/isinfo.php).
The build script uses `vswhere` to locate the default Visual Studio x64
redistributable `vcruntime140.dll`. If discovery is unavailable, supply
`-VcRuntimePath` with its full path under `VC/Redist/MSVC/<version>/x64/`.
Use a redistributable matching or newer than the compiler used to build Cash Ledger;
the script rejects a DLL for a different CPU architecture.
Run in PowerShell from the repository root:

```powershell
./packaging/windows/build-installer.ps1
# If the compiler is outside a standard installation directory:
./packaging/windows/build-installer.ps1 -IsccPath 'C:/Tools/Inno Setup 6/ISCC.exe'
```

The script builds a locked release for `x86_64-pc-windows-msvc`, reads the
version from Cargo metadata, compiles the installer, and prints its SHA-256.
Output is `dist/CashLedger-<version>-windows-x64-setup.exe`, with `LICENSE`,
`THIRD_PARTY_NOTICES.md`, `THIRD_PARTY_LICENSES.txt` and `SHA256SUMS.txt`.
License notices are also included inside the installer. Build and installer
failures stop the script. `target/` and `dist/` are ignored build outputs.
SQLite and GPUI component assets are already bundled in the executable.
The installer includes `vcruntime140.dll` beside it because the release imports
that runtime. Windows supplies the Universal CRT and ICU. Local runtime
deployment avoids an administrator-level prerequisite install; future Cash Ledger
releases must include runtime security updates. Redistribute only files from
the licensed Visual Studio redistributable directory, under its license terms.
Inno Setup requests that commercial users [purchase a license](https://jrsoftware.org/ishelp/topic_purchase.htm).
Do not include databases, development overrides, or arbitrary files from the
build directory in the installer.

The installer writes to `%LOCALAPPDATA%/Programs/Till`, creates a Start Menu
shortcut, offers an optional desktop shortcut, and registers an uninstaller.
It requires no administrator privileges. Keep its AppId unchanged across
versions so upgrades reuse the installation. The legacy directory name is retained
for compatibility. Shortcuts use Cash Ledger; upgrades remove the old Till shortcuts.
The installer writes `cash-ledger-installation.ini` beside the executable, with
`[Installation]` and `AppId=707A222C-758B-4E20-A86E-3A577E428B95`, to identify
managed installations. Uninstall removes this installation marker.

App-initiated updates pass `/UPDATEFROMAPP /VERYSILENT /SUPPRESSMSGBOXES /SP-
/NORESTART /NOCLOSEAPPLICATIONS /RESTARTEXITCODE=3010`. The application exits
before installation; `AppMutex=Local\CashLedger-707A222C-758B-4E20-A86E-3A577E428B95` blocks updates while any
other GUI instance still runs. No force-close or deferred executable replacement
is permitted. The update launch entry runs once after successful installation;
its `postinstall` flag suppresses launch if a system restart is required, and
its omission of `skipifsilent` permits silent update relaunch. Ordinary interactive
installs retain the launch checkbox. The installer owns successful relaunch;
the update helper must not launch a second copy.

The application database lives in its separate user-local data directory
documented in [the UI guide](ui.md). Uninstall deliberately preserves it;
there are no delete directives for user data. Back up valuable records before
testing updates. No installer is run automatically by the build script.

## Icon and executable metadata

`assets/till.svg` defines a cream T and drawer line in the existing green
palette. `assets/till.png` is its preview. `assets/till.ico` contains 32-bit
transparent images at 16, 20, 24, 32, 40, 48, 64, 128 and 256 pixels.
Regenerate the PNG and ICO after changing the matching geometry in
`packaging/windows/generate-icon.ps1`:

```powershell
./packaging/windows/generate-icon.ps1
```

`build.rs` compiles `packaging/windows/app.rc` on Windows and embeds icon
resource ID 1, which the pinned GPUI Windows platform loads for its window.
Executable file/product versions come from Cargo, and version details identify
Cash Ledger. A resource compilation failure is fatal. Release builds use the Windows
GUI subsystem to avoid a console window; debug builds retain console diagnostics.

## Verification and distribution

Run `cargo fmt --all -- --check`, `cargo check --locked --all-targets`, and
`cargo test --locked`. Build the installer using the script above, then check
the executable's Properties > Details and its Explorer, taskbar and shortcut
icons. For native smoke tests set `CASH_REGISTER_DB` to a temporary database
before launch; never use production records for packaging tests.

On a clean Windows test machine, install, open Cash Ledger, save and reopen a register,
install the same version again, and uninstall. Verify shortcuts and uninstall
registration are removed while the database remains. The existing native
accessibility shutdown issue described in the UI guide is still applicable.
Validate runtime dependencies on a machine without development tools before
distributing broadly.

The generated installer and app are unsigned. Public distribution should sign
both with an Authenticode certificate and timestamp, then verify their
signatures. Signing credentials are not part of this repository.

### Validation on October 8, 2026

On the development Windows host, formatting, all-target checking, Clippy with
warnings denied, and all 27 tests passed. The locked optimized x64 build and
Inno Setup 6.5.3 compilation succeeded. PE inspection verified the x64 GUI
subsystem, all nine embedded icon sizes at ID 1, and Till version metadata.
The icon received a static review at 16, 32, 48 and 256 pixels.

An isolated per-user installation and same-version reinstall verified both
shortcuts and uninstall registration. Hidden native startup created a temporary
SQLite database, supplied the window class icon, and loaded `vcruntime140.dll`
from the installation directory. The initial visible-window assertion was
inapplicable to a hidden launch; direct window inspection confirmed startup.
The app closed, and uninstall removed its executable, runtime, shortcuts and
registration while preserving the separate temporary database. No production
database was used. A separate clean-machine run and signing remain unverified.

## Research

- [Microsoft: icon resources and multiple resolutions](https://learn.microsoft.com/en-us/windows/win32/menurc/about-icons)
- [Microsoft: application icon construction](https://learn.microsoft.com/en-us/windows/apps/design/iconography/app-icon-construction)
- [embed-resource 3.0.12: resource compilation and error handling](https://docs.rs/embed-resource/3.0.12/embed_resource/)
- [Inno Setup: non-administrative installation](https://jrsoftware.org/ishelp/topic_admininstallmode.htm)
- [Inno Setup: stable AppId](https://jrsoftware.org/ishelp/topic_setup_appid.htm)
- [Inno Setup: update command-line switches](https://jrsoftware.org/ishelp/topic_setupcmdline.htm)
- [Inno Setup: launch flags and reboot suppression](https://jrsoftware.org/ishelp/topic_runsection.htm)
- [Inno Setup: running application mutex](https://jrsoftware.org/ishelp/topic_setup_appmutex.htm)
- [Inno Setup: uninstall icon](https://jrsoftware.org/ishelp/topic_setup_uninstalldisplayicon.htm)
- [Inno Setup: architecture identifiers](https://jrsoftware.org/ishelp/topic_archidentifiers.htm)
- [Microsoft: determining redistributable DLLs and local deployment](https://learn.microsoft.com/en-us/cpp/windows/determining-which-dlls-to-redistribute?view=msvc-170)
- [Microsoft: Windows ICU availability](https://learn.microsoft.com/en-us/windows/win32/intl/international-components-for-unicode--icu-)
