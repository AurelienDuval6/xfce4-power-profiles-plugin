//! Panel button with popup menu for power profile selection.
//!
//! The popup is a real `GtkMenu` (shown via the C shim's
//! `xfce_panel_plugin_popup_menu()`), so it gets the theme's native menu
//! chrome (background, border, shadow) and positioning/dismissal behavior
//! for free — the same mechanism plugins like xfce4-pulseaudio-plugin use.
//!
//! The slider lives inside a single `GtkMenuItem`. `GtkMenuShell` holds the
//! pointer grab while a menu is open and dispatches button/motion events to
//! the active item itself rather than letting them propagate to nested
//! children, so a plain child `GtkScale` never receives them and can't run
//! its own click/drag handling. Forwarding synthetic events into the scale
//! via `gtk_widget_event()` (as xfce4-pulseaudio-plugin's `XfpaScaleMenuItem`
//! does in C) turned out to be unreliable here — `GtkRange`'s internal
//! button/motion handling in this GTK version only accepted about 1 in 30
//! forwarded events, presumably due to state it tracks against the event's
//! original window. Instead, [`value_at`] computes the target value directly
//! from the click/drag position and sets it on the scale, sidestepping
//! `GtkRange`'s internal event handling entirely. Mark icons
//! (`power-profile-*-symbolic`) are placed at scale tick positions using a
//! [`gtk::Fixed`] overlay for precise alignment.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;

use gtk::prelude::*;

/// Approximate padding, in pixels, between a [`gtk::Scale`]'s allocation edge
/// and its trough. Used both here (to map a click position to a value) and in
/// [`PowerProfilesWidget::reposition_marks`] (to place mark icons), so clicks
/// line up with where the icons visually sit.
const TROUGH_PAD: f64 = 12.0;

/// Minimum width, in pixels, requested for the popup's scale.
///
/// `scale`'s real allocated width always ends up a little more than this —
/// see [`PowerProfilesWidget::reposition_marks`] for why that extra amount
/// is measured live each show rather than hardcoded, and
/// [`PowerProfilesWidget::trough_width`] for the mechanism.
const SCALE_WIDTH: f64 = 180.0;

/// Maps a profile name to its standard Adwaita symbolic icon name.
fn profile_icon(name: &str) -> &str {
    match name {
        "power-saver" => "power-profile-power-saver-symbolic",
        "performance" => "power-profile-performance-symbolic",
        _ => "power-profile-balanced-symbolic",
    }
}

#[cfg(test)]
mod tests {
    use super::{profile_icon, value_from_trough_x};

    #[test]
    fn maps_trough_center_to_middle_value() {
        assert_eq!(value_from_trough_x(114.0, 228.0, 0.0, 2.0), 1.0);
    }

    #[test]
    fn clamps_positions_before_the_leading_pad_to_the_lower_bound() {
        assert_eq!(value_from_trough_x(0.0, 228.0, 0.0, 2.0), 0.0);
    }

    #[test]
    fn clamps_positions_past_the_trailing_pad_to_the_upper_bound() {
        assert_eq!(value_from_trough_x(228.0, 228.0, 0.0, 2.0), 2.0);
    }

    #[test]
    fn maps_known_power_saver_profile() {
        assert_eq!(
            profile_icon("power-saver"),
            "power-profile-power-saver-symbolic"
        );
    }

    #[test]
    fn maps_known_performance_profile() {
        assert_eq!(
            profile_icon("performance"),
            "power-profile-performance-symbolic"
        );
    }

    #[test]
    fn maps_balanced_to_balanced_icon() {
        assert_eq!(profile_icon("balanced"), "power-profile-balanced-symbolic");
    }

    #[test]
    fn falls_back_to_balanced_icon_for_unknown_profiles() {
        assert_eq!(
            profile_icon("custom-backend-profile"),
            "power-profile-balanced-symbolic"
        );
    }

    #[test]
    fn falls_back_to_balanced_icon_for_empty_name() {
        assert_eq!(profile_icon(""), "power-profile-balanced-symbolic");
    }
}

// C shim function for xfce_panel_plugin_popup_menu().
//
// Handles alignment, auto-hide locking, and the native positioning/dismissal
// behavior of a GtkMenu.
extern "C" {
    fn plugin_popup_menu(plugin: *mut c_void, menu: *mut c_void, widget: *mut c_void);
}

/// Maps an x position within a scale's allocation to an adjustment value,
/// clamping to `[lower, upper]` rather than failing for positions in the
/// [`TROUGH_PAD`] margins at either edge.
fn value_from_trough_x(x: f64, alloc_width: f64, lower: f64, upper: f64) -> f64 {
    let trough_w = (alloc_width - 2.0 * TROUGH_PAD).max(1.0);
    let frac = ((x - TROUGH_PAD) / trough_w).clamp(0.0, 1.0);
    lower + frac * (upper - lower)
}

/// Computes the scale's adjustment value for a click/drag at `(event_x,
/// event_y)`, given in `item`'s coordinate space. Returns `None` if the
/// position falls outside the scale's own allocation.
fn value_at(item: &gtk::MenuItem, scale: &gtk::Scale, event_x: f64, event_y: f64) -> Option<f64> {
    let (sx, sy) = item.translate_coordinates(scale, event_x as i32, event_y as i32)?;
    let alloc = scale.allocation();
    if sx <= 0 || sx >= alloc.width() || sy <= 0 || sy >= alloc.height() {
        return None;
    }

    let adj = scale.adjustment();
    Some(value_from_trough_x(
        f64::from(sx),
        f64::from(alloc.width()),
        adj.lower(),
        adj.upper(),
    ))
}

/// Internal widget state. Wrapped in `Rc<RefCell<>>` for shared ownership.
struct Inner {
    button: gtk::Button,
    image: gtk::Image,
    menu: gtk::Menu,
    scale: gtk::Scale,
    mark_icons: Vec<gtk::Image>,
    profiles: Vec<String>,
    on_selected: Option<Rc<dyn Fn(i32)>>,
    plugin: *mut c_void,
}

/// Tracks [`PowerProfilesWidget::trough_width`] as it settles — see that
/// field's doc comment.
#[derive(Clone, Copy)]
enum TroughWidth {
    Unread,
    Pending(f64),
    Settled(f64),
}

/// Panel widget with button, popup menu, and D-Bus integration.
///
/// Cloneable via `Rc` (not deep clone). The `updating` flag is a separate
/// `Cell<bool>` outside the `RefCell<Inner>` to prevent re-entrant borrow
/// conflicts when `set_value` triggers `value_changed` callbacks.
#[derive(Clone)]
pub struct PowerProfilesWidget {
    inner: Rc<RefCell<Inner>>,
    updating: Rc<Cell<bool>>,
    mark_fixed: gtk::Fixed,
    /// `scale`'s real settled allocated width, captured live once it stops
    /// changing between calls and reused from then on — see
    /// [`PowerProfilesWidget::reposition_marks`] for why it's captured this
    /// way rather than read fresh every time or hardcoded as a constant.
    trough_width: Rc<Cell<TroughWidth>>,
}

impl PowerProfilesWidget {
    /// Creates the panel button and popup menu with a horizontal scale.
    pub fn new(plugin: *mut c_void) -> Self {
        let image = gtk::Image::from_icon_name(
            Some("power-profile-balanced-symbolic"),
            gtk::IconSize::SmallToolbar,
        );
        let button = gtk::Button::new();
        button.set_image(Some(&image));
        button.set_relief(gtk::ReliefStyle::None);
        button.set_focus_on_click(false);
        button.set_tooltip_text(Some("Balanced"));

        let adjustment = gtk::Adjustment::new(1.0, 0.0, 2.0, 1.0, 1.0, 0.0);
        let scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&adjustment));
        scale.set_draw_value(false);
        scale.set_hexpand(true);
        scale.set_size_request(SCALE_WIDTH as i32, -1);

        let mark_fixed = gtk::Fixed::new();
        mark_fixed.set_halign(gtk::Align::Fill);

        let popup_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        popup_box.set_margin_start(4);
        popup_box.set_margin_end(4);
        popup_box.set_margin_top(2);
        popup_box.set_margin_bottom(2);
        popup_box.pack_start(&scale, true, true, 0);
        popup_box.pack_start(&mark_fixed, false, false, 0);

        let item = gtk::MenuItem::new();
        item.add(&popup_box);
        // Menu items only request enter/leave events by default (for hover
        // highlighting); continuous drag updates need motion events too.
        item.add_events(
            gtk::gdk::EventMask::POINTER_MOTION_MASK | gtk::gdk::EventMask::BUTTON_MOTION_MASK,
        );
        // Tracks whether the primary button is held down over the item, so
        // motion events only drag the slider during an actual click-drag.
        let dragging = Rc::new(Cell::new(false));
        {
            let scale = scale.clone();
            let dragging = dragging.clone();
            item.connect_button_press_event(move |item, event| {
                if event.button() == 1 {
                    dragging.set(true);
                    let (x, y) = event.position();
                    if let Some(value) = value_at(item, &scale, x, y) {
                        scale.set_value(value);
                    }
                }
                glib::Propagation::Stop
            });
        }
        {
            let dragging = dragging.clone();
            item.connect_button_release_event(move |_, event| {
                if event.button() == 1 {
                    dragging.set(false);
                }
                glib::Propagation::Stop
            });
        }
        {
            let scale = scale.clone();
            item.connect_motion_notify_event(move |item, event| {
                if dragging.get() {
                    let (x, y) = event.position();
                    if let Some(value) = value_at(item, &scale, x, y) {
                        scale.set_value(value);
                    }
                }
                glib::Propagation::Stop
            });
        }

        let menu = gtk::Menu::new();
        // GtkMenuShell reserves a "toggle size" — space for a checkbox or
        // radio indicator — before each item's left edge by default, sized
        // to whatever the widest such indicator among the menu's items
        // would need, so mixed plain/check/radio items still line up. This
        // menu only ever has the one plain item, but the reservation still
        // applies unconditionally, unaffected by
        // GtkMenuItem::reserve-indicator (that only concerns *this* item's
        // own indicator, not the menu-wide aggregate). Disabling it here is
        // what actually fixes the asymmetric margin.
        menu.set_reserve_toggle_size(false);
        menu.append(&item);
        menu.show_all();

        // GtkMenuShell grabs keyboard input while the menu is open and
        // handles arrow keys itself for item-to-item navigation — a no-op
        // here since there's only one item, and it never reaches `scale`'s
        // own default GtkRange key bindings. Handle Left/Right/Up/Down
        // directly and stop the event so GtkMenuShell's built-in navigation
        // doesn't otherwise swallow it.
        {
            let scale = scale.clone();
            menu.connect_key_press_event(move |_, event| {
                let adj = scale.adjustment();
                let delta = match event.keyval() {
                    gtk::gdk::keys::constants::Left | gtk::gdk::keys::constants::Down => -1.0,
                    gtk::gdk::keys::constants::Right | gtk::gdk::keys::constants::Up => 1.0,
                    _ => return glib::Propagation::Proceed,
                };
                scale.set_value((scale.value() + delta).clamp(adj.lower(), adj.upper()));
                glib::Propagation::Stop
            });
        }

        let inner = Inner {
            button,
            image,
            menu,
            scale,
            mark_icons: Vec::new(),
            profiles: Vec::new(),
            on_selected: None,
            plugin,
        };

        let widget = Self {
            inner: Rc::new(RefCell::new(inner)),
            updating: Rc::new(Cell::new(false)),
            mark_fixed,
            trough_width: Rc::new(Cell::new(TroughWidth::Unread)),
        };

        // Reposition mark icons whenever the scale is resized.
        {
            let this = widget.clone();
            widget
                .inner
                .borrow()
                .scale
                .connect_size_allocate(move |s, _| {
                    this.reposition_marks(s);
                });
        }

        widget.setup_signals();
        widget
    }

    /// Recalculates mark icon positions based on the scale's trough geometry.
    ///
    /// Icons are placed in a `gtk::Fixed` overlay. Positions are computed
    /// from [`Self::trough_width`] — `scale`'s real allocated width, once it
    /// settles — using the same [`TROUGH_PAD`] approximation [`value_at`]
    /// uses, so the icons line up with where clicks register.
    ///
    /// `scale`'s real allocated width always ends up a little more than
    /// [`SCALE_WIDTH`] (menu/menu-item chrome pads it further, even with
    /// `menu.set_reserve_toggle_size(false)` in `new()` — see that call's
    /// doc comment for the *other*, larger and asymmetric, padding problem
    /// it fixes). That extra amount isn't a fixed constant: it was measured
    /// at 180, 194, 198, and 202px across separate panel restarts — a
    /// same-theme, same-machine spread wide enough to visibly throw off
    /// mark positions if hardcoded (icons drifting further right the higher
    /// their value, since `power-saver` sits at the trough's zero point
    /// regardless of width but `balanced`/`performance` don't).
    ///
    /// So this reads the live allocation instead — but not on every call.
    /// `mark_fixed` is a sibling of `scale` in the same box, and a
    /// `GtkFixed`'s minimum width always equals its natural width — both are
    /// simply `max(child.x + child.width)`, with no "needs" vs. "would
    /// like" distinction a wrapping container could otherwise cap. So
    /// whatever `mark_fixed` requests here leaks into the shared box's
    /// width, which widens `scale`'s next allocation, which would get read
    /// back in on the *next* call if read live unconditionally —
    /// compounding a little further on every open (the menu and its
    /// children are created once and reused, so the drift never resets on
    /// its own); confirmed empirically as a live reproduction of the
    /// original growth bug. Reading until the value repeats, then freezing
    /// there for good, gets the precision of a live read (tracking
    /// whatever this session's real width happens to be) without an
    /// unbounded read ever remaining in the loop to compound.
    fn reposition_marks(&self, scale: &gtk::Scale) {
        let inner = self.inner.borrow();
        let icons = &inner.mark_icons;
        let n = icons.len();
        if n == 0 {
            return;
        }

        let adj = scale.adjustment();
        let lower = adj.lower();
        let upper = adj.upper();
        let range = upper - lower;
        if range <= 0.0 {
            return;
        }

        let scale_width = match self.trough_width.get() {
            TroughWidth::Settled(w) => w,
            TroughWidth::Unread => {
                let w = f64::from(scale.allocation().width());
                if w > 0.0 {
                    self.trough_width.set(TroughWidth::Pending(w));
                }
                w
            }
            TroughWidth::Pending(prev) => {
                let w = f64::from(scale.allocation().width());
                if w > 0.0 {
                    self.trough_width.set(if (w - prev).abs() < f64::EPSILON {
                        TroughWidth::Settled(w)
                    } else {
                        TroughWidth::Pending(w)
                    });
                }
                w
            }
        };
        let trough_w = scale_width - 2.0 * TROUGH_PAD;

        for (i, icon) in icons.iter().enumerate() {
            let v = i as f64;
            let px = ((v - lower) / range).mul_add(trough_w, TROUGH_PAD);
            self.mark_fixed.move_(icon, (px - 8.0).max(0.0) as i32, 0);
        }
    }

    /// Connects button click and scale value-changed signals.
    fn setup_signals(&self) {
        // Button click → show popup via xfce_panel_plugin_popup_menu().
        {
            let this = self.clone();
            self.inner.borrow().button.connect_clicked(move |_| {
                let inner = this.inner.borrow();
                unsafe {
                    plugin_popup_menu(
                        inner.plugin,
                        inner.menu.as_ptr().cast::<c_void>(),
                        inner.button.as_ptr().cast::<c_void>(),
                    );
                }
            });
        }

        // Scale value-changed → snap to nearest integer position and notify.
        {
            let this = self.clone();
            self.inner.borrow().scale.connect_value_changed(move |s| {
                if this.updating.get() {
                    return;
                }
                let snapped = s.value().round();
                if (s.value() - snapped).abs() > f64::EPSILON {
                    this.updating.set(true);
                    s.set_value(snapped);
                    this.updating.set(false);
                    return;
                }
                let pos = snapped as usize;
                let inner = this.inner.borrow();
                if let (Some(cb), true) = (inner.on_selected.as_ref(), pos < inner.profiles.len()) {
                    cb(pos as i32);
                }
            });
        }
    }

    /// Registers a callback invoked when the user selects a profile.
    pub fn connect_profile_selected<F: Fn(i32) + 'static>(&self, f: F) {
        self.inner.borrow_mut().on_selected = Some(Rc::new(f));
    }

    /// Updates the scale range and mark icons to match available profiles.
    ///
    /// Removes existing marks and icons, rebuilds them for the new profile list.
    /// Called when `Profiles` D-Bus property changes.
    pub fn update_profiles(&self, profiles: &[String]) {
        self.updating.set(true);
        let mut inner = self.inner.borrow_mut();
        let profiles = profiles.to_vec();
        inner.profiles = profiles.clone();

        for child in self.mark_fixed.children() {
            self.mark_fixed.remove(&child);
        }
        inner.mark_icons.clear();

        if profiles.is_empty() {
            inner.scale.set_sensitive(false);
        } else {
            inner.scale.set_sensitive(true);
            inner
                .scale
                .adjustment()
                .set_upper((profiles.len() as f64) - 1.0);
            for i in 0..profiles.len() {
                inner
                    .scale
                    .add_mark(i as f64, gtk::PositionType::Bottom, None);
            }
            for name in &profiles {
                let icon = gtk::Image::from_icon_name(
                    Some(profile_icon(name)),
                    gtk::IconSize::SmallToolbar,
                );
                self.mark_fixed.put(&icon, 0, 0);
                inner.mark_icons.push(icon);
            }
            self.mark_fixed.show_all();
        }
        self.updating.set(false);
    }

    /// Updates the active profile icon, tooltip, and scale position.
    pub fn set_active_profile(&self, name: &str) {
        self.updating.set(true);
        let inner = self.inner.borrow_mut();

        if let Some(pos) = inner.profiles.iter().position(|p| p == name) {
            inner.scale.set_value(pos as f64);
        }

        inner
            .image
            .set_from_icon_name(Some(profile_icon(name)), gtk::IconSize::SmallToolbar);
        inner.button.set_tooltip_text(Some(name));
        self.updating.set(false);
    }

    /// Returns the panel button widget to be added to the panel container.
    #[must_use]
    pub fn panel_widget(&self) -> gtk::Button {
        self.inner.borrow().button.clone()
    }

    /// Returns the current list of available profile names.
    #[must_use]
    pub fn available_profiles(&self) -> Vec<String> {
        self.inner.borrow().profiles.clone()
    }
}
