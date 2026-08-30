// C shim header for xfce4-panel plugin.
// Exposes functions callable from Rust via FFI.
#ifndef __POWER_PROFILES_PLUGIN_H__
#define __POWER_PROFILES_PLUGIN_H__

// XFCE panel plugin constructor — called by wrapper-2.0 on load.
void constructor(void *plugin);
// Popup menu helper — wraps xfce_panel_plugin_popup_menu().
void plugin_popup_menu(void *plugin, void *menu, void *widget);

#endif
