# Framework concepts

## Application, state, and handles

`Application` initializes and runs GPUI. `App` provides access to application
state and services. A `Window` provides window-specific operations. The root
of a window is an entity whose type implements `Render`.

An `Entity<T>` is a typed handle to state managed by GPUI. Cloning the handle
does not clone the underlying `T`. Read state with the appropriate context and
modify it through an update closure. Keep ordinary business data in conventional
Rust types; use entities where state needs to participate in GPUI's lifecycle
and communication system.

Use weak entity handles for relationships that should not keep a view alive,
particularly callbacks and asynchronous completion paths. A weak handle can
fail to upgrade after its owner is released. Treat that as an expected lifecycle
case, not necessarily an application failure. Strong reference cycles can keep
entities alive, just as cycles can with reference-counted ownership elsewhere.

## Context selection

| Type | Responsibility |
| --- | --- |
| `App` | Application services and access to entities |
| `Context<T>` | Entity-specific operations, including notifications, listeners, and subscriptions; also dereferences to `App` |
| `Window` | Layout, painting, focus, and other operations for a particular window |
| `AsyncApp` | Application access across asynchronous suspension points |
| `AsyncWindowContext` | Async access associated with a window |
| `TestAppContext` | Framework test services and simulated execution |

Choose parameters based on the operation required. Do not retain borrowed
`&mut App`, `&mut Window`, or `&mut Context<T>` across an `await`.
Async contexts can outlive the objects they address, so their fallible operations
must be handled. Upstream's [context guide](https://github.com/zed-industries/zed/blob/main/crates/gpui/docs/contexts.md)
explains the distinctions; check local signatures for version-specific details.

## Views and elements

`Render::render` receives `&mut self`, a window, and an entity context, and returns
something implementing `IntoElement`. The result describes the UI for that
render pass. Persistent application state belongs in the view or model, not in
a temporary local variable recreated on each render.

`div()` is a general container. `child` appends one child and `children` appends
multiple children. Strings can be children. The builder methods are Rust trait
methods, not HTML attributes. Import `gpui::prelude::*` when using a focused
import list so the relevant extension traits are available.

The hybrid model can be understood as:

```text
Persistent entity state
        -> render produces elements
        -> layout / prepaint / paint
        -> GPU output
Input -> event or action handler -> state update -> notification -> next render
```

This is a conceptual flow, not a guarantee of a fixed refresh frequency.
Avoid disk access, network work, heavyweight calculations, and repeated
subscription creation inside `render`.

## Layout and units

Layout is configured through Rust methods such as `flex`, `flex_col`,
`items_center`, `justify_center`, and explicit dimension methods. `px` expresses
a logical pixel length. GPUI also distinguishes device pixels, scaled pixels,
relative units, and percentages. Do not equate logical pixels with physical
display pixels when handling high-DPI displays.

`size_full` depends on the space supplied by its parent. Scroll containers need
meaningful bounds; unbounded content cannot behave like a bounded viewport.
Test resizing and long text instead of relying only on a fixed initial window.
These are API mechanics; visual design should follow the project's chosen UI
skills and design system when actual interface work is requested.

## Identity and state changes

Give interactive/stateful elements stable IDs. IDs compose with ancestor IDs
to form global element identities. Changing an ID changes the identity across
frames; duplicate global identities in one frame can corrupt state association.
Prefer an item's stable domain identifier to its current position when rows
can be reordered.

Mutating data and notifying GPUI are separate operations. After a visible
state change, call `cx.notify()` so the entity's observers and dependent UI
can respond. `observe` reacts to notifications; typed events emitted through
`emit` and received through `subscribe` describe a particular occurrence.
An event emitter implements `EventEmitter<EventType>` for the relevant type.

The installed `Subscription` cancels its callback when dropped. Store handles
on the owning view or controller, and release them with that owner. Detaching
a subscription intentionally allows it to continue until the relevant entity
lifecycle ends; do not detach merely to silence an unused-value diagnostic.

## Components and boundaries

Use `Render` for stateful views and `RenderOnce` for owned, reusable composition
where appropriate. Reach for `Element` or a canvas only when ordinary containers
cannot express the required layout or painting. Custom elements require more
care with layout, input, identity, and accessibility.

Keep small applications simple. As features grow, separate domain logic from
views and platform integration. Create services and modules when they have an
actual responsibility; this starter does not need a speculative architecture.
