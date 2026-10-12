# Setup and repository walkthrough

## Dependency identity and versions

The following values were read from `Cargo.toml` and `Cargo.lock` during research:

| Dependency | Manifest requirement | Locked version | Rust import |
| --- | --- | --- | --- |
| GPUI | `gpui-unofficial = "=1.19.0-pre"` | `1.19.0-pre` | `gpui` |
| Platform | package `gpui-platform-gpui-unofficial`, version `"=1.19.0-pre"` | `1.19.0-pre` | `gpui_platform` |
| Components | package `gpui-component-uo`, version `"=1.19.0-pre"` | `1.19.0-pre` | `gpui_component` |
| Compatibility workaround | `tinyvec = "=1.12.0"` | `1.12.0` | Not directly used in the application |

The package is `cash-counter` version `0.0.1`, using Rust edition 2024.
The repository pins the validated Rust 1.96.0 toolchain in `rust-toolchain.toml`.
The research machine reported Rust and Cargo 1.96.0; this is an observed
toolchain, not a proven minimum requirement.

The GPUI packages use exact compatible pins; see [the UI guide](../ui.md) for
the component compatibility decision. `--locked` prevents lockfile changes.
The platform package's manifest itself depends on a compatible GPUI range;
different package version numbers do not by themselves imply duplicate GPUI
types. Inspect `cargo tree --duplicates` when investigating dependency conflicts.

The local GPUI manifest declares its library as `gpui`, which explains why
`use gpui::*;` works despite the package being named `gpui-unofficial`.
The project's explicit platform dependency alias supplies `gpui_platform`.
The unofficial distribution is maintained separately from Zed; see its
[publisher repository](https://github.com/iamnbutler/gpui-unofficial).

The existing `tinyvec` pin cites
[upstream issue 225](https://github.com/Lokathor/tinyvec/issues/225).
Preserve it until an intentional dependency change has demonstrated that the
workaround is no longer needed. This documentation change does not alter it.

## Native prerequisites

Use stable Rust with support for edition 2024 and the selected dependencies.
Check the actual compiler diagnostic before deciding whether an upgrade is needed.

| Platform | Setup considerations |
| --- | --- |
| Windows | Use an appropriate native Rust target and matching linker. For the usual MSVC target, install Visual Studio Build Tools with C++ tools and a Windows SDK. The installed platform implementation selects Win32 services. No additional platform feature is needed specifically for Windows. |
| Linux | The project enables both `wayland` and `x11`. A desktop session and native development libraries required by the selected backends are needed. Consult the current upstream Linux build guide and the dependency's diagnostics; distribution package names vary. |
| macOS | Upstream documents Xcode and its command-line tools for Metal. The platform crate also has a `font-kit` feature for text rendering; this project's platform dependency currently enables only `wayland` and `x11`. Verify the resolved macOS text features before claiming macOS readiness. |

These are setup considerations, not a claim that this project was tested on
every platform. The installed platform source dispatches by target OS;
Linux feature names do not make a Windows build use Linux windowing.
See [Zed's development docs](https://zed.dev/docs/development) and the
[GPUI introduction](https://gpui.rs/) for upstream entry points.

## Commands

Run from the project root:

```sh
rustc --version
cargo --version
cargo check --locked
cargo run --locked
cargo build --release --locked
cargo doc --locked --no-deps
```

Use `cargo doc --locked` when you want local dependency documentation as well.
The resulting `target/doc/` tree is generated output, not maintained source docs.
`--offline` can be added when the required packages are already cached; an
offline cache miss does not establish a code defect.

If your environment uses RTK, prefix these commands with `rtk`, for example
`rtk cargo check --locked`. RTK is an environment convenience, not an app runtime
dependency or a prerequisite for another developer.

## Repository layout

```text
cash-counter/
  Cargo.toml          Package and dependency requirements
  Cargo.lock          Resolved dependency versions
  src/main.rs         Application startup, assets, theme, and key bindings
  src/ui/             Register workspace and forms
  src/register/       Background operations and exact amount parsing
  src/database/       SQLite storage and migrations
  README.md           Project entry point
  docs/gpui/          Maintained framework and development guide
  target/             Generated Cargo output
```

## Existing execution path

1. `main` calls `gpui_platform::application()` to select a platform-backed
   `Application`.
2. `run` starts the event loop and passes an application context to its closure.
3. Startup initializes component assets, the light theme, and keyboard bindings.
4. `cx.open_window` creates the register workspace inside the component `Root`.
5. The view loads persisted data through a background `RegisterService` task.
   Window-creation failures are reported and terminate startup cleanly.
