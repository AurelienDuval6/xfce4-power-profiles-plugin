//! Panel button with popup slider for power profile selection.
//!
//! The popup is a plain `GtkWindow` shown via the C shim's
//! `xfce_panel_plugin_popup_window()`, which handles positioning, auto-hide
//! locking, click-outside dismissal and Wayland layer-shell. It carries
//! `GDK_WINDOW_TYPE_HINT_POPUP_MENU`, so it sets the same
//! `_NET_WM_WINDOW_TYPE_POPUP_MENU` atom a `GtkMenu` does and window
//! managers and compositors treat it as a context menu rather than an
//! application window (undecorated, out of the taskbar and Alt-Tab, and
//! animated with whatever rule the compositor applies to menus).
//!
//! Keeping it an ordinary window means the `GtkScale` inside is an ordinary
//! child widget: it handles its own clicks, drags and arrow keys through
//! `GtkRange`'s built-in event handling, with no forwarding, no
//! position-to-value mapping and no key glue of our own. Mark icons
//! (`power-profile-*-symbolic`) are placed at scale tick positions using a
//! [`gtk::Fixed`] overlay for precise alignment.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;

use gtk::prelude::*;

/// Approximate padding, in pixels, between a [`gtk::Scale`]'s allocation edge
/// and its trough. Used by [`PowerProfilesWidget::reposition_marks`] to place
/// the mark icons over the tick positions the scale draws.
const TROUGH_PAD: f64 = 12.0;

/// Half the width of a [`gtk::IconSize::SmallToolbar`] icon, subtracted from a
/// mark's position so the tick lands on the icon's centre rather than its left
/// edge.
const MARK_ICON_HALF_WIDTH: f64 = 8.0;

/// Minimum width, in pixels, requested for the popup's scale. The real
/// allocated width ends up a little more than this, since the popup's own
/// margins pad it further, which is why
/// [`PowerProfilesWidget::reposition_marks`] positions the mark icons from
/// the live allocation rather than from this constant.
const SCALE_WIDTH: i32 = 180;

/// Maps a profile name to its standard Adwaita symbolic icon name.
fn profile_icon(name: &str) -> &str {
    match name {
        "power-saver" => "power-profile-power-saver-symbolic",
        "performance" => "power-profile-performance-symbolic",
        _ => "power-profile-balanced-symbolic",
    }
}

/// X offset, in pixels, of the mark icon standing for `v` on a scale whose
/// adjustment runs from `lower` to `upper` and which is `alloc_w` pixels wide.
///
/// Returns `None` when the adjustment has no span, which is the case for a
/// single profile and for the placeholder range in place before the first
/// profile list arrives from D-Bus.
///
/// The trough is taken to sit [`TROUGH_PAD`] in from each edge of the scale's
/// allocation, and the result is pulled left by [`MARK_ICON_HALF_WIDTH`] so a
/// tick lands on its icon's centre. Values outside the adjustment clamp to the
/// trough ends rather than being placed beyond them.
fn mark_offset(v: f64, lower: f64, upper: f64, alloc_w: i32) -> Option<i32> {
    let range = upper - lower;
    if range <= 0.0 {
        return None;
    }
    // Clamped because the scale can be allocated before it is ever shown, when
    // its width may not cover the trough padding on both sides.
    let trough_w = (f64::from(alloc_w) - 2.0 * TROUGH_PAD).max(0.0);
    let px = ((v - lower) / range).clamp(0.0, 1.0) * trough_w + TROUGH_PAD;
    Some((px - MARK_ICON_HALF_WIDTH) as i32)
}

#[cfg(test)]
mod tests {
    use super::{mark_offset, profile_icon, MARK_ICON_HALF_WIDTH, TROUGH_PAD};

    /// A settled popup allocates its 180px scale request plus 8px of margin on
    /// each side.
    const SETTLED: i32 = 196;

    /// X offset of the leftmost mark: the trough's left edge, less half an icon.
    fn left_end() -> i32 {
        (TROUGH_PAD - MARK_ICON_HALF_WIDTH) as i32
    }

    /// X offset of the rightmost mark of a scale `alloc_w` wide, which depends
    /// only on the trough's right edge.
    fn right_end(alloc_w: i32) -> i32 {
        (f64::from(alloc_w) - TROUGH_PAD - MARK_ICON_HALF_WIDTH) as i32
    }

    #[test]
    fn first_and_last_marks_sit_on_the_trough_ends() {
        assert_eq!(mark_offset(0.0, 0.0, 2.0, SETTLED), Some(left_end()));
        assert_eq!(
            mark_offset(2.0, 0.0, 2.0, SETTLED),
            Some(right_end(SETTLED))
        );
    }

    #[test]
    fn marks_are_evenly_spaced_and_ascending() {
        let offsets: Vec<i32> = (0..3)
            .map(|i| mark_offset(i as f64, 0.0, 2.0, SETTLED).expect("span is 2"))
            .collect();
        assert!(offsets[0] < offsets[1] && offsets[1] < offsets[2]);
        assert_eq!(offsets[1] - offsets[0], offsets[2] - offsets[1]);
    }

    #[test]
    fn narrow_allocations_collapse_the_trough_never_left_of_zero() {
        // Widths at and below the 2 * TROUGH_PAD the trough is inset by: the
        // trough collapses to zero and every mark lands on the same offset.
        for alloc_w in [0, 1, 12, 23, 24] {
            let first = mark_offset(0.0, 0.0, 2.0, alloc_w).expect("span is 2");
            for v in 0..3 {
                let x = mark_offset(f64::from(v), 0.0, 2.0, alloc_w).expect("span is 2");
                assert!(x >= 0, "negative offset {x} at width {alloc_w}, value {v}");
                assert_eq!(x, first, "trough should collapse at width {alloc_w}");
            }
        }
    }

    #[test]
    fn a_range_with_no_span_yields_no_offsets() {
        // One profile: lower == upper.
        assert_eq!(mark_offset(0.0, 0.0, 0.0, SETTLED), None);
        // Inverted range.
        assert_eq!(mark_offset(1.0, 0.0, -1.0, SETTLED), None);
    }

    #[test]
    fn out_of_range_values_clamp_to_the_trough_ends() {
        assert_eq!(mark_offset(-1.0, 0.0, 2.0, SETTLED), Some(left_end()));
        assert_eq!(
            mark_offset(9.0, 0.0, 2.0, SETTLED),
            Some(right_end(SETTLED))
        );
    }

    #[test]
    fn the_first_mark_is_pinned_while_the_last_follows_the_width() {
        // The trough padding is a fixed inset, so growing the scale must not
        // shift the low end — only stretch the span.
        assert_eq!(
            mark_offset(0.0, 0.0, 2.0, 320),
            mark_offset(0.0, 0.0, 2.0, SETTLED)
        );
        assert!(mark_offset(2.0, 0.0, 2.0, 320).expect("span is 2") > right_end(SETTLED));
    }

    #[test]
    fn marks_sit_where_the_scale_draws_its_ticks() {
        // The scale's own value at the midpoint of its allocation is halfway
        // between the marks, so the mark offsets must bracket the trough's
        // midpoint symmetrically.
        let mid = mark_offset(1.0, 0.0, 2.0, SETTLED).expect("span is 2");
        let low = left_end();
        let high = right_end(SETTLED);
        assert_eq!(low + high, 2 * mid);
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

// C shim function for xfce_panel_plugin_popup_window().
//
// Handles popup positioning, auto-hide locking, click-outside dismissal,
// and Wayland layer-shell.
extern "C" {
    fn plugin_popup_window(plugin: *mut c_void, window: *mut c_void, widget: *mut c_void);
}

/// Internal widget state. Wrapped in `Rc<RefCell<>>` for shared ownership.
struct Inner {
    button: gtk::Button,
    image: gtk::Image,
    popup: gtk::Window,
    scale: gtk::Scale,
    mark_icons: Vec<gtk::Image>,
    profiles: Vec<String>,
    on_selected: Option<Rc<dyn Fn(i32)>>,
    plugin: *mut c_void,
}

/// Panel widget with button, popup slider, and D-Bus integration.
///
/// Cloneable via `Rc` (not deep clone). The `updating` flag is a separate
/// `Cell<bool>` outside the `RefCell<Inner>` to prevent re-entrant borrow
/// conflicts when `set_value` triggers `value_changed` callbacks.
#[derive(Clone)]
pub struct PowerProfilesWidget {
    inner: Rc<RefCell<Inner>>,
    updating: Rc<Cell<bool>>,
    mark_fixed: gtk::Fixed,
}

impl PowerProfilesWidget {
    /// Creates the panel button and popup window with a horizontal scale.
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
        scale.set_size_request(SCALE_WIDTH, -1);

        let mark_fixed = gtk::Fixed::new();
        mark_fixed.set_halign(gtk::Align::Fill);

        let popup_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        popup_box.set_margin_start(8);
        popup_box.set_margin_end(8);
        popup_box.set_margin_top(6);
        popup_box.set_margin_bottom(6);
        popup_box.pack_start(&scale, true, true, 0);
        // Packed without expanding, and deliberately kept from widening the
        // popup. mark_fixed's natural width is whatever the icon positions
        // reach, and those are computed by reposition_marks() from the scale's
        // allocation — so anything that lets that allocation grow widens the
        // popup, which widens the allocation again. That loop is what made the
        // popup grow on every open before. It holds because the scale asks for
        // SCALE_WIDTH plus the margins either side (196px) while the marks need
        // at most `alloc - 4`, leaving 4px of headroom once settled. Widening
        // the margins, lowering SCALE_WIDTH or enlarging the icons closes that
        // gap and brings the loop back.
        popup_box.pack_start(&mark_fixed, false, false, 0);

        let popup = gtk::Window::new(gtk::WindowType::Toplevel);
        // Same _NET_WM_WINDOW_TYPE atom a GtkMenu sets, so window managers
        // and compositors classify this as a context menu rather than an
        // application window. This is what xfce_panel_plugin_popup_window()
        // would otherwise leave as GDK_WINDOW_TYPE_HINT_UTILITY.
        popup.set_type_hint(gtk::gdk::WindowTypeHint::PopupMenu);
        // xfce_panel_plugin_popup_window() sets GDK_WINDOW_TYPE_HINT_UTILITY
        // itself, overwriting the hint above, so re-apply it from ::show —
        // which runs before the window is mapped, so the property is already
        // correct by the time the compositor classifies the window.
        popup.connect_show(|w| w.set_type_hint(gtk::gdk::WindowTypeHint::PopupMenu));
        popup.set_decorated(false);
        popup.set_skip_taskbar_hint(true);
        popup.set_skip_pager_hint(true);
        popup.add(&popup_box);

        // Initial show/hide so the window is realized before popup_window()
        // positions it.
        popup.show_all();
        popup.hide();

        let inner = Inner {
            button,
            image,
            popup,
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
    /// Icons are placed in a `gtk::Fixed` overlay, each at the offset
    /// [`mark_offset`] gives for its position in the profile list. Reading the
    /// scale's live allocation rather than a constant is what keeps the icons
    /// aligned across themes and panel sizes, since the scale is allocated
    /// slightly wider than the width it requests.
    ///
    fn reposition_marks(&self, scale: &gtk::Scale) {
        let inner = self.inner.borrow();
        let icons = &inner.mark_icons;
        if icons.is_empty() {
            return;
        }

        let adj = scale.adjustment();
        let (lower, upper) = (adj.lower(), adj.upper());
        let alloc_w = scale.allocation().width();

        for (i, icon) in icons.iter().enumerate() {
            if let Some(x) = mark_offset(i as f64, lower, upper, alloc_w) {
                self.mark_fixed.move_(icon, x, 0);
            }
        }
    }

    /// Connects button click and scale value-changed signals.
    fn setup_signals(&self) {
        // Button click → show popup via xfce_panel_plugin_popup_window().
        {
            let this = self.clone();
            self.inner.borrow().button.connect_clicked(move |_| {
                let inner = this.inner.borrow();
                unsafe {
                    plugin_popup_window(
                        inner.plugin,
                        inner.popup.as_ptr().cast::<c_void>(),
                        inner.button.as_ptr().cast::<c_void>(),
                    );
                }
                // The scale is the only focusable widget in the popup, and
                // arrow keys only reach it once it holds the keyboard focus.
                // Grabbing from ::show instead would also fire on the
                // construction-time show_all() in new(), and would run before
                // the window is mapped, where a focus request is merely
                // recorded rather than applied.
                inner.scale.grab_focus();
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
                    // A drag can land between ticks. GtkRange's round-digits
                    // defaults to -1, which derives its rounding precision
                    // from the adjustment's step increment, so a value like
                    // 1.7 really does reach this handler. Snapping re-enters
                    // here with an integral value, and it is that nested call
                    // which notifies — so `updating` must not be raised to
                    // suppress it, and this frame must not notify a second
                    // time.
                    s.set_value(snapped);
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
        inner.profiles = profiles.to_vec();

        for child in self.mark_fixed.children() {
            self.mark_fixed.remove(&child);
        }
        inner.mark_icons.clear();
        // `add_mark` appends rather than replaces, so without this the ticks
        // from every earlier profile list survive and stack up as duplicates
        // on the same positions.
        inner.scale.clear_marks();

        let scale = inner.scale.clone();

        if profiles.is_empty() {
            scale.set_sensitive(false);
        } else {
            scale.set_sensitive(true);
            scale.adjustment().set_upper((profiles.len() as f64) - 1.0);
            for i in 0..profiles.len() {
                scale.add_mark(i as f64, gtk::PositionType::Bottom, None);
            }
            for name in profiles {
                let icon = gtk::Image::from_icon_name(
                    Some(profile_icon(name)),
                    gtk::IconSize::SmallToolbar,
                );
                self.mark_fixed.put(&icon, 0, 0);
                inner.mark_icons.push(icon);
            }
            self.mark_fixed.show_all();
        }

        // The icons are placed at the origin and normally moved by
        // `size_allocate`, but the scale is already at its final width when
        // the profile list changes while the popup is closed, and GTK skips
        // the allocation when nothing resized. Placing them here covers that
        // case. The mutable borrow has to go first: `reposition_marks` takes
        // its own shared borrow of `inner`.
        drop(inner);
        self.reposition_marks(&scale);

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
