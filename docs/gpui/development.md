# Rust development patterns

API details in this document were checked against the installed GPUI 1.23.2
source, particularly `src/app/context.rs`, `src/subscription.rs`, and the shipped
examples. The following counter is an illustrative standalone program, not a
modification of `src/main.rs` and not a complete accessible button component.

## Updating view state

```rust
use gpui::{Context, IntoElement, Render, Window, WindowOptions, div, prelude::*};

#[derive(Default)]
struct Counter {
    value: u64,
}

impl Render for Counter {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("counter")
            .child(format!("Count: {}", self.value))
            .on_click(cx.listener(|this, _event, _window, cx| {
                // Keep repeated input bounded rather than overflowing the counter.
                this.value = this.value.saturating_add(1);
                cx.notify();
            }))
    }
}

fn main() {
    gpui_platform::application().run(|cx| {
        if let Err(error) = cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| Counter::default())
        }) {
            eprintln!("Unable to open the application window: {error}");
            cx.quit();
        }
    });
}
```

`cx.listener` adapts an element callback into a callback that can update this
view. Its installed signature supplies the view, event, window, and typed
context. It internally uses a weak view handle. The handler changes persistent
state and notifies GPUI; the next render uses the updated value.

For deployed applications, route failures through your established reporting
system and review the content of diagnostics. The example reports a local
startup error to the process console.

## Keyboard actions and focus

Define semantic actions with `actions!` or the supported action derive/attribute
for the installed version. Register bindings with `cx.bind_keys`, for example:

```rust
use gpui::{App, KeyBinding, actions};

actions!(counter, [Increment]);

fn register_bindings(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("enter", Increment, Some("Counter"))]);
}
```

This fragment only registers a binding. A working control also needs a retained
`FocusHandle`, the correct focused dispatch path, a matching
`.key_context("Counter")`, and an `.on_action(cx.listener(...))` handler.
Route pointer and keyboard paths into the same state-changing operation.
Use `track_focus` and explicit focus management as shown by installed examples.

Upstream's [key dispatch guide](https://github.com/zed-industries/zed/blob/main/crates/gpui/docs/key_dispatch.md)
explains actions and contexts. Some examples there use Zed-specific JSON
configuration; a standalone Rust app can register bindings directly.
Verify callback signatures against the installed source before copying older
examples. Test focus transitions, Tab navigation, shortcut conflicts, and
visible focus feedback.

## Asynchronous work

The installed `Context::spawn` accepts an async closure receiving a weak entity
and a mutable async application context. `spawn_in` additionally associates
work with a window. Background executor tasks require suitable `Send` data;
foreground tasks operate through the application's executor.

Use this sequence when loading data:

1. Set a loading state, start a bounded operation, and notify the view.
2. Move owned input into the task; do not capture a borrowed mutable view.
3. Perform blocking or expensive work away from rendering and event callbacks.
4. Handle the operation's result, then update through the weak entity and async
   context. The entity may already have been released.
5. Apply the result only if it still belongs to the latest request. Notify the
   view and expose an appropriate success, empty, or error state.

Retain the returned task for an owned lifecycle, await it, or intentionally
detach it. The local API explicitly requires holding or detaching tasks.
Define cancellation and error handling before detaching work. Framework async
support does not automatically supply an external service's timeout, retry,
authentication, or runtime requirements. Verify a network client's executor
requirements separately before introducing it.

## Text and editable input

`SharedString` is useful for immutable display text and inexpensive sharing.
Creating a text child displays text; it does not create an editable field.

Editable input needs selection, cursor movement, clipboard behavior, focus,
composition, and platform IME integration. The installed `examples/input.rs`
uses `EntityInputHandler`, `ElementInputHandler`, shaped text, and selection
types. Study that example rather than treating `on_key_down` as a complete
text editor. Respect UTF-8 character boundaries and the platform's selection
index conventions; bytes, Unicode scalar values, graphemes, and UTF-16 offsets
are different units.

## Assets, scrolling, and custom drawing

GPUI supplies image and SVG elements, asset integration, scroll handles,
virtualized list facilities, and low-level painting APIs. Use local reference
docs for the installed `AssetSource`, image, SVG, and list APIs.

Define a predictable asset root or an embedding strategy. Avoid assuming the
process working directory always equals the project root. Handle missing and
invalid assets explicitly. Cache reusable resources outside hot render paths
and bound cache growth. For large lists, render the visible range with the
appropriate list abstraction and preserve stable row identity.

Use a canvas or custom element when direct painting is necessary. Drawing a
shape does not automatically supply input semantics or accessibility metadata.

## Accessibility

The installed `_accessibility.rs` guide describes AccessKit integration. Stable
element IDs and roles are part of exposing a node to assistive technology.
Use the installed `examples/a11y.rs` and related support code to implement
roles, accessible names, values, states, and actions appropriate to each control.

A clickable `div` alone does not establish button semantics, keyboard support,
or an accessible name. The counter above demonstrates state flow only. Before
shipping a control, add keyboard activation, focus feedback, and the relevant
accessibility behavior. Also verify readable contrast, text scaling, reduced
motion, and screen-reader behavior on each supported platform.

## Error handling and data boundaries

Represent meaningful application states explicitly, such as loading, ready,
empty, and failed. Keep errors observable without logging secrets or private
payloads. Validate external input before it becomes application state. UI
visibility is not authorization. Persistence and external services need their
own secure boundaries; the framework does not implement those policies for you.
