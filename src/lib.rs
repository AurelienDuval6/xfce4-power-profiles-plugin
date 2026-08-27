//! XFCE4 panel plugin for managing power profiles.
//!
//! This crate provides a panel plugin that communicates with
//! `org.freedesktop.UPower.PowerProfiles` via D-Bus. It supports
//! power-profiles-daemon, TLP 1.9+, and system76-power backends.
//!
//! # Architecture
//!
//! - **C shim** (`plugin.c`): Required for `XFCE_PANEL_PLUGIN_REGISTER` macro
//! - **D-Bus** (`dbus/`): Background thread with tokio runtime, events sent via `mpsc`
//! - **UI** (`ui/`): GTK3 panel button with popup slider

pub mod dbus;
pub mod ui;

use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use glib::translate::FromGlibPtrNone;
use gtk::prelude::*;

use crate::dbus::{DbusEvent, DbusManager};
use crate::ui::slider::PowerProfilesWidget;

/// Entry point called by the XFCE panel via the C shim.
///
/// # Safety
/// `pointer` is a `XfcePanelPlugin*` passed from the C shim.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn constructor(pointer: *mut c_void) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        unsafe { constructor_inner(pointer) };
    }));
}

/// Inner constructor. Wrapped in `catch_unwind` to prevent panics from
/// crossing the FFI boundary back into C.
unsafe fn constructor_inner(pointer: *mut c_void) {
    gtk::set_initialized();

    let plugin_obj: glib::Object =
        unsafe { glib::Object::from_glib_none(pointer.cast::<gobject_sys::GObject>()) };

    let container: gtk::Container = plugin_obj
        .clone()
        .downcast()
        .expect("Plugin pointer is not a GtkContainer");

    let (tx, rx) = mpsc::channel();
    let dbus_mgr = Rc::new(RefCell::new(DbusManager::new(tx)));

    let widget = PowerProfilesWidget::new(pointer);
    let panel_btn = widget.panel_widget();

    container.add(&panel_btn);
    panel_btn.show_all();

    // Poll D-Bus events from the background thread's channel.
    // Uses 50ms interval as a balance between latency and CPU usage.
    {
        let widget = widget.clone();
        let btn = panel_btn.clone();
        gtk::glib::timeout_add_local(Duration::from_millis(50), move || {
            while let Ok(event) = rx.try_recv() {
                match event {
                    DbusEvent::ActiveProfileChanged(name) => {
                        widget.set_active_profile(&name);
                    }
                    DbusEvent::ProfilesChanged(profiles) => {
                        widget.update_profiles(&profiles);
                    }
                    DbusEvent::PerformanceDegraded(degraded) => {
                        let tip = if degraded.is_empty() {
                            None
                        } else {
                            Some(format!("Performance degraded: {degraded}"))
                        };
                        btn.set_tooltip_text(tip.as_deref());
                    }
                    DbusEvent::Unavailable(msg) => {
                        widget.update_profiles(&[]);
                        btn.set_tooltip_text(Some(&msg));
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    // Forward slider position to D-Bus set_profile call.
    {
        let w = widget.clone();
        widget.connect_profile_selected(move |pos| {
            if let Some(name) = w.available_profiles().get(pos as usize) {
                dbus_mgr.borrow().set_profile(name);
            }
        });
    }

    // Scale button to match panel icon size when panel size changes.
    {
        plugin_obj.connect_local("size-changed", false, move |values| {
            if let Ok(size) = values[1].get::<i32>() {
                panel_btn.set_size_request(size, size);
            }
            Some(true.to_value())
        });
    }
}
