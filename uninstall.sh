#!/bin/bash
set -e

PLUGIN_DIR="$HOME/.local/lib/xfce4/panel/plugins"
DESKTOP_DIR="$HOME/.local/share/xfce4/panel/plugins"

echo "=== Removing xfce4-power-profiles-plugin ==="

rm "$PLUGIN_DIR/libpowerprofiles.so"
rm "$DESKTOP_DIR/power-profiles.desktop"

echo "Removed xfce4-power-profiles-plugin successfully"
