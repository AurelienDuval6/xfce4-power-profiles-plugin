# Changelog

## 2026-10-04 — v0.2.0 Popup window behaviour and slider interaction

Release summary for the 0.2.0 tag. The dated sections below remain the detailed
session log; this one states the net effect on the shipped plugin.

No new features, no configuration or file-layout changes, nothing to migrate.

### Changes
- The popup carries `GDK_WINDOW_TYPE_HINT_POPUP_MENU`, set before it is realized and re-applied from `::show` because `xfce_panel_plugin_popup_window()` overwrites it with `GDK_WINDOW_TYPE_HINT_UTILITY`. WMs and compositors therefore classify it as a context menu rather than an application window — undecorated, kept out of the taskbar and Alt-Tab, stacked with other menus, and dismissed by click-outside as a menu. Verified as `_NET_WM_WINDOW_TYPE_POPUP_MENU` on the mapped window
- The decorated / skip-taskbar / skip-pager hints are now set explicitly rather than left to the panel API. Most WMs latch onto a window's type and decoration hints at first map, which is why hints applied after realization had no effect
- Kept as a plain `GtkWindow` rather than a `GtkMenu`. WMs classify a popup on the `_NET_WM_WINDOW_TYPE` atom, which `gtk_window_set_type_hint()` sets to the same value a `GtkMenu` does, so the scale stays an ordinary child widget handling its own clicks, drags and arrow keys through `GtkRange`'s built-in event handling. No event forwarding, position-to-value mapping or key glue of our own
- `SCALE_WIDTH` 250 → 180. The popup is narrower
- The scale takes keyboard focus when the popup opens, so Left/Right/Up/Down step between profiles. The grab moved onto the button-click path: from `::show` it also fired on the construction-time `show_all()` and ran before the window was mapped, where a focus request is recorded rather than applied
- Mark icon placement extracted into a pure `mark_offset()` function (`value`, adjustment bounds, allocated width → x offset). `reposition_marks()` now only reads coordinates and moves widgets

### Fixes
- Releasing a drag between two ticks now selects the nearest profile. The snap path raised `updating` around `set_value()`, which suppressed the nested `value-changed` carrying the notification, then returned without notifying itself — so `on_selected` never fired. `GtkRange`'s `round-digits` defaults to -1 and takes its precision from the adjustment's step increment, so off-tick values like 1.7 do reach the handler; this is the main path a mouse drag takes
- Scale ticks no longer accumulate. `gtk_scale_add_mark()` appends and `update_profiles()` never cleared them, so every `ProfilesChanged` signal and every daemon reconnect stacked another set on the same positions, and an empty profile list left the stale ones drawn
- Mark icons are placed directly at the end of `update_profiles()`, not only from the scale's `size_allocate`. Changing the profile list while the popup was closed left them stacked at the origin, because GTK skips an allocation whose size has not changed
- Trough geometry is clamped: the trough width can no longer go negative before the scale is first allocated, and values outside the adjustment clamp to the trough ends instead of being placed beyond them

### Internals
- `mark_fixed` stays packed without expanding, so the mark overlay cannot widen the popup. Its natural width is whatever the icon positions reach, and those are computed from the scale's allocation, so anything that lets that allocation grow feeds back into the next repositioning. The invariant holds while the scale requests `SCALE_WIDTH` plus 8px either side against marks reaching at most `alloc - 4`, and is recorded in `decisions.md` so a change to the margins, `SCALE_WIDTH` or the icon size does not reopen the loop silently
- Dropped the trough-width settle state machine, the CSS provider and its `power-profiles-popup` style class, and a redundant profile-vector clone in `update_profiles()`

### Tests
- Seven unit tests over `mark_offset()`, covering the cases that broke in the field: allocation too narrow to cover the trough padding, an adjustment with no span (one profile, or the placeholder range before the first D-Bus list), values outside the bounds, even spacing, and the invariant that the low mark stays pinned while the high mark follows the width. The clamps they cover are load-bearing — verified by mutation

### Docs
- `decisions.md`: popup chrome comes from the theme's `window` node. The CSS provider that faked the `menu` node is not to be reintroduced — `border-radius` on an undecorated window is the construct the 2026-08-26 entry rejected, and the type hint only fixes how a window is classified, not how it is composited
- `decisions.md`: the mark overlay must not be able to widen the popup, with the 4px of headroom that keeps it so
- `.gitignore`: the local `.cargo/config.toml` (clang + mold linker) is kept out of the tree. It broke CI, where mold is installed on neither the `ubuntu-latest` build job nor the `debian:trixie` package job

### Known limitations
- The popup renders with the theme's `window` node: sharp corners, theme background and border. Rounded corners stay unreachable until the ARGB constraint behind the black-corner artifact under xfwm4 is resolved

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
