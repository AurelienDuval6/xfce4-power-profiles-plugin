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

/// Minimum width, in pixels, requested for the popup's scale. The real
/// allocated width ends up a little more than this, since the popup's own
/// margins pad it further, which is why
/// [`PowerProfilesWidget::reposition_marks`] positions the mark icons from
/// the live allocation rather than from this constant.
const SCALE_WIDTH: f64 = 180.0;

/// Menu-like chrome for the popup window, drawn from the theme's own named
/// colours so it follows the user's GTK theme rather than hardcoding one.
/// A `GtkMenu` gets this from the theme's `menu` CSS node for free; a plain
/// window's node is `window`, which themes style as an application window.
const POPUP_CSS: &str = "
window.power-profiles-popup {
    background-color: @theme_bg_color;
    border: 1px solid alpha(@theme_fg_color, 0.25);
    border-radius: 4px;
}
";

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
    use super::profile_icon;

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
        scale.set_size_request(SCALE_WIDTH as i32, -1);

        let mark_fixed = gtk::Fixed::new();
        mark_fixed.set_halign(gtk::Align::Fill);

        let popup_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        popup_box.set_margin_start(8);
        popup_box.set_margin_end(8);
        popup_box.set_margin_top(6);
        popup_box.set_margin_bottom(6);
        popup_box.pack_start(&scale, true, true, 0);
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
        popup.style_context().add_class("power-profiles-popup");
        popup.add(&popup_box);

        if let Some(screen) = gtk::gdk::Screen::default() {
            let provider = gtk::CssProvider::new();
            if provider.load_from_data(POPUP_CSS.as_bytes()).is_ok() {
                gtk::StyleContext::add_provider_for_screen(
                    &screen,
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            }
        }

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
    /// Icons are placed in a `gtk::Fixed` overlay. Positions are computed
    /// from [`Self::trough_width`] — `scale`'s real allocated width, once it
    /// settles — using a [`TROUGH_PAD`] approximation of where the trough
    /// sits inside the scale's allocation, so the icons line up with the
    /// tick positions the scale draws.
    ///
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

        let trough_w = f64::from(scale.allocation().width()) - 2.0 * TROUGH_PAD;

        for (i, icon) in icons.iter().enumerate() {
            let v = i as f64;
            let px = ((v - lower) / range).mul_add(trough_w, TROUGH_PAD);
            self.mark_fixed.move_(icon, (px - 8.0).max(0.0) as i32, 0);
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
                // The scale is the only focusable widget in the popup;
                // focusing it explicitly is what makes arrow keys work.
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
        inner.profiles = profiles.to_vec();

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
