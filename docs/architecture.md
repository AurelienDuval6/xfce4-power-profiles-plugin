# Architecture

## Overview

The plugin is a shared library (`.so`) loaded by the XFCE panel's `wrapper-2.0` process. It consists of three layers: a thin C shim, a Rust entry point, and the UI/D-Bus modules.

## Component Diagram

```
┌─────────────────────────────────────────────────────┐
│                  xfce4-panel (host)                  │
│  ┌─────────────────────────────────────────────────┐│
│  │              wrapper-2.0 process                ││
│  │  ┌───────────────────────────────────────────┐  ││
│  │  │         libpowerprofiles.so               │  ││
│  │  │                                           │  ││
│  │  │  ┌──────────┐  ┌────────┐  ┌──────────┐  │  ││
│  │  │  │ plugin.c │──│ lib.rs │──│  dbus/   │  │  ││
│  │  │  │ (C shim) │  │        │  │  mod.rs  │  │  ││
│  │  │  └──────────┘  │        │  │  proxy.rs│  │  ││
│  │  │                │        │  └────┬─────┘  │  ││
│  │  │                │        │       │        │  ││
│  │  │                │        │  ┌────┴─────┐  │  ││
│  │  │                │        │  │  ui/     │  │  ││
│  │  │                │        │  │ slider.rs│  │  ││
│  │  │                │        │  └──────────┘  │  ││
│  │  │                └────────┘                │  ││
│  │  └───────────────────────────────────────────┘  ││
│  └─────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────┘
                         │
                    D-Bus (system)
                         │
                         ▼
            ┌─────────────────────────┐
            │  org.freedesktop.UPower  │
            │   .PowerProfiles        │
            │                         │
            │  power-profiles-daemon  │
            │  / TLP / system76-power │
            └─────────────────────────┘
```

## Data Flow

### Profile Change (user)
1. User clicks panel button → popup shown with scale widget
2. User drags scale to new position
3. `slider.rs` fires `on_selected` callback with position index
4. `lib.rs` callback resolves position to profile name via `available_profiles()`
5. `DbusManager::set_profile()` spawns a tokio runtime on a new thread
6. zbus calls `SetActiveProfile` D-Bus property setter on `org.freedesktop.UPower.PowerProfiles`

### Profile Change (external)
1. `DbusManager` runs a tokio runtime on a dedicated background thread
2. On startup, reads current `ActiveProfile` and `Profiles` via D-Bus
3. Subscribes to `PropertiesChanged` signals via `zbus::MessageStream`
4. When `ActiveProfile` changes, sends `DbusEvent::ActiveProfileChanged` through `mpsc::channel`
5. `lib.rs` polls the channel every 50ms on the GTK main loop
6. `PowerProfilesWidget::set_active_profile()` updates icon, tooltip, and scale position

## Tech Stack

| Layer | Technology |
|---|---|
| Panel integration | C (`libxfce4panel-2.0`) — C shim required for `XFCE_PANEL_PLUGIN_REGISTER` macro |
| Entry point | Rust (`extern "C"` constructor, glib-rs) |
| UI | GTK3 via `gtk-rs` 0.18 |
| D-Bus | zbus 5 (async), tokio runtime |
| Channel | `std::sync::mpsc` bridging tokio → GTK main loop |

## Key Design Decisions

- **Staticlib + C shim**: XFCE panel requires a `XFCE_PANEL_PLUGIN_REGISTER` macro at the C level. The Rust code is compiled as a staticlib and linked with the C shim into a single `.so`.
- **Separate tokio runtime**: D-Bus operations run on a dedicated background thread with their own tokio runtime, avoiding interference with GTK's main loop.
- **Polling bridge**: D-Bus events are sent via `mpsc::channel` and polled every 50ms on the GTK main loop via `timeout_add_local`. This avoids sending GTK operations from async contexts.
- **`Rc<RefCell<Inner>>` + `Rc<Cell<bool>>`**: The widget is cloneable via `Rc` (not deep clone). The `updating` flag is a separate `Cell<bool>` outside the `RefCell<Inner>` to prevent re-entrant borrow conflicts when `set_value` triggers `value_changed` callbacks.
- **`xfce_panel_plugin_popup_window()` for popup management**: Uses the xfce4-panel 4.19+ API for popup positioning, auto-hide locking, and click-outside dismissal. Handles X11 and Wayland (via layer-shell) transparently. The C shim wraps the macro call since it requires XFCE panel types.
