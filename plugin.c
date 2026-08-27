// C shim for xfce4-panel plugin registration.
// The XFCE_PANEL_PLUGIN_REGISTER macro cannot be called from Rust.
#include <libxfce4panel/libxfce4panel.h>
#include "plugin.h"

XFCE_PANEL_PLUGIN_REGISTER(constructor);

// Blocks/resumes panel auto-hide while the popup is open.
void plugin_block_autohide(gpointer plugin, gboolean block) {
    xfce_panel_plugin_block_autohide(XFCE_PANEL_PLUGIN(plugin), block);
}

// Wraps xfce_panel_plugin_popup_window() — handles positioning, auto-hide
// locking, click-outside dismissal, and Wayland layer-shell.
void plugin_popup_window(gpointer plugin, gpointer window, gpointer widget) {
    xfce_panel_plugin_popup_window(XFCE_PANEL_PLUGIN(plugin), GTK_WINDOW(window), GTK_WIDGET(widget));
}
