# GPUI guide for this project

Research date: **2026-10-08**. Scope: native desktop applications implemented
in Rust, using this repository's dependency resolution.

This is a practical framework and project guide, not a replacement for the full
API reference. Examples use Rust for UI, state, events, and startup; no HTML,
JavaScript, CSS files, webview, or frontend build pipeline is required.
Native operating-system and graphics libraries are still dependencies: writing
the application in Rust does not mean its entire platform stack is Rust.

## Reading order

1. [Setup and repository walkthrough](setup.md): packages, prerequisites, build
   commands, and the existing application's execution path.
2. [Framework concepts](concepts.md): entities, contexts, rendering, layout,
   notifications, ownership, and component boundaries.
3. [Rust development patterns](development.md): state updates, events, keyboard
   actions, asynchronous work, text, assets, and accessibility.
4. [Validation and troubleshooting](validation.md): checks, tests, manual
   verification, common failures, and release considerations.
5. [Research sources and version policy](sources.md): authoritative references,
   evidence boundaries, and how to keep these documents current.

## What GPUI provides

GPUI combines persistent application state with a UI tree produced by Rust
render functions. It draws that tree using a GPU and supplies windowing,
input dispatch, text services, and executors through its platform integration.
It powers the Zed editor. The framework offers both declarative composition
through views and lower-level control through custom elements.
See the [official introduction](https://gpui.rs/) and
[upstream source](https://github.com/zed-industries/zed/tree/main/crates/gpui).

GPUI is evolving quickly. A community package's `1.x` release number does not
establish that upstream GPUI has reached a stable 1.0 API. Do not mix examples
from the official package, community mirrors, and Zed's moving `main` branch
without checking the installed signatures.

## Current project scope

The application now has a GPUI Component register workspace, editable inputs,
SQLite persistence, a background storage service, and database and UI tests.
See [the UI guide](../ui.md) for current behavior and dependency pins. The general
framework notes were originally researched against GPUI 1.23.2; this application
uses the compatible 1.19.0-pre component stack, so verify exact signatures against
the installed source. Release packaging is not configured.
