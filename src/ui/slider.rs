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
//! children, so a plain child `GtkScale` would never receive them. Instead,
//! button/motion events on the item are manually re-targeted at the scale
//! (see [`forward_button_event`] / [`forward_motion_event`]), mirroring
//! xfce4-pulseaudio-plugin's `XfpaScaleMenuItem`. Mark icons
//! (`power-profile-*-symbolic`) are placed at scale tick positions using a
//! [`gtk::Fixed`] overlay for precise alignment.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;

use glib::translate::ToGlibPtrMut;
use gtk::prelude::*;

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

// C shim function for xfce_panel_plugin_popup_menu().
//
// Handles alignment, auto-hide locking, and the native positioning/dismissal
// behavior of a GtkMenu.
extern "C" {
    fn plugin_popup_menu(plugin: *mut c_void, menu: *mut c_void, widget: *mut c_void);
}

/// Forwards a button event to `scale` if it lands within the scale's
/// allocation, translating its coordinates from `item`'s space into the
/// scale's own before re-delivering it.
fn forward_button_event(item: &gtk::MenuItem, scale: &gtk::Scale, event: &gtk::gdk::EventButton) {
    let (x, y) = event.position();
    let Some((sx, sy)) = item.translate_coordinates(scale, x as i32, y as i32) else {
        return;
    };
    let alloc = scale.allocation();
    if sx <= 0 || sx >= alloc.width() || sy <= 0 || sy >= alloc.height() {
        return;
    }
    let mut translated = event.clone();
    unsafe {
        let raw = translated.to_glib_none_mut().0;
        (*raw).x = sx as f64;
        (*raw).y = sy as f64;
    }
    scale.event(&translated);
}

/// Forwards a motion event to `scale` if it lands within the scale's
/// allocation, translating its coordinates from `item`'s space into the
/// scale's own before re-delivering it. Needed for continuous drag updates.
fn forward_motion_event(item: &gtk::MenuItem, scale: &gtk::Scale, event: &gtk::gdk::EventMotion) {
    let (x, y) = event.position();
    let Some((sx, sy)) = item.translate_coordinates(scale, x as i32, y as i32) else {
        return;
    };
    let alloc = scale.allocation();
    if sx <= 0 || sx >= alloc.width() || sy <= 0 || sy >= alloc.height() {
        return;
    }
    let mut translated = event.clone();
    unsafe {
        let raw = translated.to_glib_none_mut().0;
        (*raw).x = sx as f64;
        (*raw).y = sy as f64;
    }
    scale.event(&translated);
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
        scale.set_size_request(250, -1);

        let mark_fixed = gtk::Fixed::new();
        mark_fixed.set_halign(gtk::Align::Fill);

        let popup_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        popup_box.set_margin_start(8);
        popup_box.set_margin_end(8);
        popup_box.set_margin_top(6);
        popup_box.set_margin_bottom(6);
        popup_box.pack_start(&scale, true, true, 0);
        popup_box.pack_start(&mark_fixed, false, false, 0);

        let item = gtk::MenuItem::new();
        item.add(&popup_box);
        // Menu items only request enter/leave events by default (for hover
        // highlighting); continuous drag updates need motion events too.
        item.add_events(
            gtk::gdk::EventMask::POINTER_MOTION_MASK | gtk::gdk::EventMask::BUTTON_MOTION_MASK,
        );
        {
            let scale = scale.clone();
            item.connect_button_press_event(move |item, event| {
                forward_button_event(item, &scale, event);
                glib::Propagation::Stop
            });
        }
        {
            let scale = scale.clone();
            item.connect_button_release_event(move |item, event| {
                forward_button_event(item, &scale, event);
                glib::Propagation::Stop
            });
        }
        {
            let scale = scale.clone();
            item.connect_motion_notify_event(move |item, event| {
                forward_motion_event(item, &scale, event);
                glib::Propagation::Stop
            });
        }

        let menu = gtk::Menu::new();
        menu.append(&item);
        menu.show_all();

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
    /// Icons are placed in a `gtk::Fixed` overlay. Positions are computed from
    /// the scale's allocation with a 12px pad approximation for the trough edges.
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

        let alloc = scale.allocation();
        let pad = 12;
        let trough_w = alloc.width() - 2 * pad;

        for (i, icon) in icons.iter().enumerate() {
            let v = i as f64;
            let px = ((v - lower) / range).mul_add(trough_w as f64, pad as f64);
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
