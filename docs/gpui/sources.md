# Research sources and version policy

Reviewed on **2026-10-08**. Prefer the installed source for exact signatures,
then version-matched Rustdoc, then upstream guides for concepts.

## Online primary sources

| Source | Purpose and limitation |
| --- | --- |
| [GPUI official site](https://gpui.rs/) | Framework introduction and links to upstream guides. Its introductory startup example can differ from the platform split used here. |
| [Official examples index](https://gpui.rs/examples/) | Entry point for practical examples; confirm compatibility with local dependencies. |
| [Zed GPUI source](https://github.com/zed-industries/zed/tree/main/crates/gpui) | Upstream implementation and examples. `main` is moving, not the project's pinned revision. |
| [Upstream context guide](https://github.com/zed-industries/zed/blob/main/crates/gpui/docs/contexts.md) | Roles of application, entity, async, and window types. |
| [Upstream key dispatch guide](https://github.com/zed-industries/zed/blob/main/crates/gpui/docs/key_dispatch.md) | Actions, keyboard contexts, and dispatch. Includes Zed configuration examples that are not mandatory for this app. |
| [Official package Rustdoc](https://docs.rs/gpui/latest/gpui/) | Useful API comparison; the page observed reported 0.2.2 and contains older platform guidance. It is not this project's installed package. |
| [Unofficial distribution Rustdoc](https://docs.rs/gpui-unofficial/latest/gpui/) | Distribution introduction and API entry point. The page observed reported 1.22.0, older than the lockfile's 1.23.2. |
| [Unofficial publisher repository](https://github.com/iamnbutler/gpui-unofficial) | Publisher identity from the installed package metadata; separate from Zed maintenance. |
| [Zed development documentation](https://zed.dev/docs/development) | Entry point for native platform build instructions, not an exact dependency list for this smaller app. |

## Local primary evidence

The research also inspected these installed package files, obtained through
Cargo's registry cache rather than a third-party tutorial:

- `gpui-unofficial-1.23.2/Cargo.toml`: package/library identity, features, and
  example targets.
- `gpui-unofficial-1.23.2/src/app/context.rs`: notifications, listeners, and
  task spawn signatures.
- `gpui-unofficial-1.23.2/src/subscription.rs`: cancellation and detachment.
- `gpui-unofficial-1.23.2/src/_accessibility.rs`: element identity, roles,
  and AccessKit behavior.
- `gpui-unofficial-1.23.2/examples/hello_world.rs` and `examples/input.rs`:
  startup/rendering and editable text implementation.
- `gpui-platform-gpui-unofficial-1.20.2/Cargo.toml` and
  `src/gpui_platform.rs`: feature wiring and host platform selection.

Find the corresponding installed versions under your Cargo registry source
directory; its exact location and registry hash vary by machine. Generate local
Rustdoc with `cargo doc --locked` for convenient browsing.

## Evidence boundaries

The counter program and keyboard binding fragment in `development.md` were
combined in a temporary Cargo example and passed `cargo check --locked --offline`
against this project's dependencies. The unused binding helper produced an
expected dead-code warning. The temporary source was removed after validation.
Compilation does not establish runtime or accessibility behavior. Exact-version remote Rustdoc
URLs were inaccessible during this session. Where online information lagged
behind the lockfile, installed source took precedence.

Do not interpret current upstream features as automatically present in the
locked community distribution. Do not infer stable API guarantees from a
publisher's version number. Cross-platform and accessibility behavior still
require testing on the intended operating systems.

## Maintaining these docs

When dependencies change, update the research date and version table, recheck
startup and callback APIs, confirm feature wiring, validate Rust snippets, and
record the checks actually performed. Keep maintained Markdown under
`docs/gpui/`; leave generated API HTML in `target/doc/`.
