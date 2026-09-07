// C shim header for xfce4-panel plugin.
// Exposes functions callable from Rust via FFI.
#ifndef __POWER_PROFILES_PLUGIN_H__
#define __POWER_PROFILES_PLUGIN_H__

// XFCE panel plugin constructor — called by wrapper-2.0 on load.
void constructor(void *plugin);
// Popup window helper — wraps xfce_panel_plugin_popup_window().
void plugin_popup_window(void *plugin, void *window, void *widget);

#endif
