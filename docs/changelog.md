# Changelog

## 2026-08-25 — Initial implementation

### Features
- Panel button with power-profile icon that updates when the active profile changes
- Popup window with a discrete 3-position slider (Saver / Balanced / Perf.)
- D-Bus integration with `org.freedesktop.UPower.PowerProfiles` via zbus 5
- Real-time profile change monitoring via `PropertiesChanged` signals
- Automatic slider snap to discrete positions
- Icon and tooltip update on external profile changes
- Performance degradation tooltip display
- Proper auto-hide prevention via `xfce_panel_plugin_block_autohide()`
- Monitor-aware popup positioning with right-edge clamping

### Architecture
- Rust staticlib + C shim linked into a single `.so`
- Separate tokio runtime for D-Bus on a background thread
- `mpsc::channel` bridge from tokio to GTK main loop (50ms poll)
- `Rc<RefCell<Inner>>` + `Rc<Cell<bool>>` widget pattern

### Tooling
- `build.sh` / `install.sh` scripts
- GitHub Actions CI (format check, clippy, build)

## 2026-08-26 — Refinements

### Fixes
- Fixed GLib-CRITICAL NULL crash (`from_glib_full` → `from_glib_none`)
- Fixed MatchRule `.destination()` panic (changed to `.sender()`)
- Fixed RefCell double-borrow in callbacks
- Fixed popup positioning using GdkWindow origin + allocation
- Fixed popup going off-screen (monitor-aware clamping)

### Improvements
- Removed unused dependencies (`gdk`, `gdk-sys`, `libc`, `pkg-config`)
- Replaced battery icons with standard `power-profile-*-symbolic` icons
- Shortened profile labels to prevent overlap ("Saver", "Balanced", "Perf.")
- Clean build with zero compiler warnings
- Added `xfce_panel_plugin_block_autohide()` for auto-hide mode support

## 2026-08-26 — Popup API migration

### Changes
- Migrated popup from manual positioning to `xfce_panel_plugin_popup_window()` API (xfce4-panel 4.19+)
- Removed manual GdkWindow origin + allocation positioning, monitor-aware clamping, and focus-out handlers
- Removed `xfce_panel_plugin_block_autohide()` usage — now handled by `xfce_panel_plugin_popup_window()` automatically
- Dropped CSS rounded corners on popup window — ARGB transparency incompatible with `GDK_WINDOW_TYPE_HINT_UTILITY` set by the API (caused black artifacts under xfwm4)
- Removed CSS provider, `set_widget_name`, and `connect_map` RGBA visual handler
- C shim now exposes `plugin_popup_window()` alongside existing `plugin_block_autohide()`

## 2026-08-26 — Documentation and code comments

### Changes
- Added `//!` module-level doc comments to all source files (`lib.rs`, `dbus/mod.rs`, `dbus/proxy.rs`, `ui/mod.rs`, `ui/slider.rs`)
- Added `///` doc comments to all public items and key internal types
- `cargo doc --no-deps` generates clean API documentation at `target/doc/powerprofiles/index.html`
- Added "Generating Documentation" section to `docs/setup.md`

## 2026-09-25 — Popup chrome and focus

### Changes
- Popup renders with the theme's own `window` node (sharp corners) — removed the CSS provider and the `power-profiles-popup` class that only it consumed, reinstating the 2026-08-26 ARGB finding recorded in `decisions.md`
- Moved the arrow-key focus grab back onto the button-click path, after `xfce_panel_plugin_popup_window()` has shown the window. Grabbing from `::show` also fired on the construction-time `show_all()` and ran before the window was mapped, where a focus request is recorded rather than applied
- `SCALE_WIDTH` is an `i32`; it was only ever a pixel count

### Fixes
- Clamped the trough width in `reposition_marks()` to zero so mark icons no longer compute a negative trough before the scale is first allocated
- Corrected a `reposition_marks()` doc comment still referring to the `trough_width` settle machinery removed in 3509b09 (the only `cargo doc` warning left in the crate)

## 2026-09-25 — Profile list and drag fixes

### Fixes
- Cleared the scale's tick marks before rebuilding them in `update_profiles()`. `gtk_scale_add_mark()` appends, so every `ProfilesChanged` signal and every daemon reconnect stacked another set of ticks on the same positions, and an empty profile list left the old ones behind
- Releasing a drag between two ticks now selects the nearest profile. The snap path raised `updating` around `set_value()`, which suppressed the nested `value-changed` that carries the notification, then returned without notifying — so `on_selected` never fired. `GtkRange`'s `round-digits` defaults to -1 and takes its precision from the adjustment's step increment, so off-tick values like 1.7 do reach the handler
- Mark icons are now placed directly at the end of `update_profiles()`, not only from the scale's `size_allocate`. Changing the profile list while the popup was closed left them stacked at the origin, because GTK skips an allocation whose size has not changed
