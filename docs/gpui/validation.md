# Validation and troubleshooting

## Routine validation

Run relevant checks from the root:

```sh
cargo fmt --all -- --check
cargo check --locked
cargo clippy --locked --all-targets
cargo test --locked
cargo build --release --locked
```

These are development commands, not a statement that all checks have passed.
The starter has no authored tests. A successful `cargo test` without behavioral
tests provides compilation evidence, not evidence of UI correctness.
Use `--offline` only when dependencies are cached. Do not remove the lockfile
or disable validation to bypass a dependency error.

## Testing layers

Keep business calculations in ordinary Rust functions and test them with
standard unit tests. Use GPUI test contexts for entity updates, events, focus,
and application behavior. The installed crate provides `#[gpui::test]` and
`TestAppContext`; enable the appropriate `test-support` features when adding
framework tests and verify the installed example signatures.

A future development dependency can enable `test-support` on the same package
identity as the runtime dependency. Do not introduce a separate official GPUI
crate alongside the unofficial one: identically named types from different
crate instances are distinct types. This guide does not change test features.

Headless tests do not replace native window and rendering checks. The installed
platform helper's headless renderer support is platform-dependent; do not
assume GPU screenshot testing is uniformly available across operating systems.

## Manual smoke test

For this starter, run `cargo run --locked` and verify a visible window, readable
centered greeting, resizing behavior, and normal window closure. Record the
platform and graphics environment used. For interactive features, also test
pointer and keyboard activation, focus transitions, long text, loading/error
states, and release-mode behavior.

## Common symptoms

| Symptom | First investigation |
| --- | --- |
| A builder method is unavailable | Import the relevant prelude traits and check the installed API version. |
| An example imports the wrong crate | Distinguish Cargo package names, library names, and dependency aliases. |
| State changes but the UI is stale | Verify the update reaches the intended entity and calls `notify`. |
| An observer never runs | Check that the subscription handle remains alive and that the source notifies or emits the matching event. |
| Async work appears to disappear | Check task ownership, entity release, cancellation, and reported errors. |
| A keyboard shortcut does nothing | Check registration, key context, focus path, and action handler. |
| A window fails to open | Handle `open_window`'s result; inspect platform initialization and GPU diagnostics. The current app discards that result. |
| A Linux backend fails to compile | Check enabled backend features and the exact missing native dependency. |
| Text is missing on macOS | Check the platform crate's text backend features and font availability. |
| Duplicate-type errors | Inspect `cargo tree --duplicates` for incompatible versions or different GPUI distributions. |
| Linking fails on Windows | Check Rust target, linker, C++ build tools, and Windows SDK installation. |
| Offline build fails | Identify the missing cached package; retry online only when fetching dependencies is intended. |

## Releases and upgrades

`cargo build --release --locked` produces a binary; it does not create an
installer, sign the executable, bundle all assets, or implement updates.
Packaging must account for each platform's resources, metadata, and signing
requirements. None of those release facilities exist in this starter.

Upgrade GPUI packages deliberately: review distribution and upstream changes,
inspect the resolved tree, check signatures, run targeted tests and native
smoke tests, and update these docs. Preserve a recoverable manifest and lockfile
before changing dependency requirements. Avoid broad unrelated upgrades.

## Documentation verification record

Research matched the manifest, lockfile, application source, and locally cached
GPUI/platform crate source. Browser access to exact-version Rustdoc pages was
unavailable during research, so newer APIs were checked locally. The
existing application passed `cargo check --locked --offline`. The counter
program and keyboard binding fragment also passed that check when combined in
a temporary Cargo example; the uncalled binding helper produced a dead-code
warning. Local Markdown file links and balanced code fences were verified.
No native window smoke test or full test suite was run for this documentation
task. The temporary example was removed and application source was unchanged.
