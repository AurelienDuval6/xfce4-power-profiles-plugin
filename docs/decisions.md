# Architecture Decision Records

## 2026-08-25: Rust staticlib + C shim instead of pure C

**Context:** XFCE panel plugins require the `XFCE_PANEL_PLUGIN_REGISTER` macro, which is a C preprocessor macro that cannot be called from Rust.

**Decision:** Write all logic in Rust, compiled as a `staticlib`. A minimal C shim (`plugin.c`) calls the macro and declares the constructor. Both are linked into a single `.so`.

**Consequences:** Clean Rust codebase with full type safety and memory safety. The C shim is 8 lines and unlikely to need changes. The build requires both `cargo` and `gcc`.

## 2026-08-25: zbus 5 over dbus-rs

**Context:** Two main D-Bus libraries exist for Rust: `zbus` (async, Tokio-based) and `dbus-rs` (C bindings). The project needs D-Bus signal monitoring and property access.

**Decision:** Use `zbus` 5 with the `tokio` feature. It provides a high-level proxy macro that generates type-safe D-Bus bindings from trait definitions.

**Consequences:** Requires a Tokio runtime. Signal monitoring is clean via `MessageStream`. Proxy trait auto-generates getter/setter methods from D-Bus introspection.

## 2026-08-25: Separate tokio runtime on background thread

**Context:** GTK3 uses a single-threaded main loop. Tokio requires an async runtime. Sending GTK operations from async contexts causes threading issues.

**Decision:** Spawn a dedicated background thread with its own multi-threaded tokio runtime for all D-Bus operations. Use `std::sync::mpsc::channel` to send events back to the GTK main loop.

**Consequences:** Clean separation between async D-Bus work and single-threaded GTK UI. No `Send`/`Sync` constraints on GTK widgets. Trade-off is a 50ms polling interval for event delivery.

## 2026-08-25: GTK3 over GTK4

**Context:** XFCE 4.20 panel uses `libxfce4panel-2.0` which is built on GTK3. GTK4 is a separate library with incompatible APIs.

**Decision:** Use GTK3 via `gtk-rs` 0.18 with the `v3_24` feature flag.

**Consequences:** Compatible with XFCE 4.20 panel. No way to use GTK4 features. The `gtk` crate 0.18 is the final version for GTK3 (unmaintained upstream, but stable).

## 2026-08-26: xfce_panel_plugin_popup_window() for popup management

**Context:** The plugin needs a popup window anchored to the panel button. Requirements include correct positioning (especially on Wayland where layer-shell is needed), auto-hide locking, and click-outside dismissal.

**Decision:** Use `xfce_panel_plugin_popup_window()` (available since xfce4-panel 4.19.0) via a C shim wrapper. The API takes the plugin, window, and reference widget, and handles positioning, auto-hide locking, click-outside dismissal, and Wayland layer-shell transparently.

**Consequences:** Replaced ~40 lines of manual positioning, autohide, and focus-out code. Requires xfce4-panel 4.19+ (system has 4.20.8). Sets `GDK_WINDOW_TYPE_HINT_UTILITY` on the popup window, which prevents ARGB transparency (CSS rounded corners produce black artifacts). The C shim exposes `plugin_popup_window()` to wrap the macro call.

## 2026-08-26: Drop CSS rounded corners on popup

**Context:** CSS `border-radius` on an undecorated `gtk::Window` requires ARGB transparency (RGBA visual + `set_app_paintable`). However, `xfce_panel_plugin_popup_window()` sets `GDK_WINDOW_TYPE_HINT_UTILITY` on the window, which xfwm4 does not composite with ARGB transparency — resulting in a visible black rectangle behind the transparent corners.

**Decision:** Accept sharp corners. Removed CSS provider, `set_widget_name`, and `connect_map` RGBA visual handler.

**Consequences:** Popup has default GTK window appearance (sharp corners, theme-matching background). No visual artifact. Can be revisited if xfce4-panel changes the type hint or if a compositor workaround is found.

## 2026-08-26: Rc-based cloneable widget

**Context:** The XFCE panel constructor needs to share the widget between multiple signal handlers and the event polling loop.

**Decision:** Use `Rc<RefCell<Inner>>` for shared ownership and `Rc<Cell<bool>>` for the `updating` flag (separate from `RefCell` to prevent re-entrant borrow panics). The widget struct implements `Clone` via `Rc::clone`.

**Consequences:** Single-threaded only (no `Send`/`Sync`), which is fine since everything runs on the GTK main loop. The `updating` flag prevents infinite callback loops when `set_value` triggers `value_changed`.
