#!/bin/bash
set -e

PLUGIN_DIR="$HOME/.local/lib/xfce4/panel/plugins"
DESKTOP_DIR="$HOME/.local/share/xfce4/panel/plugins"

echo "=== Installing xfce4-power-profiles-plugin ==="

mkdir -p "$PLUGIN_DIR"
mkdir -p "$DESKTOP_DIR"

cp libpowerprofiles.so "$PLUGIN_DIR/"
cp power-profiles.desktop "$DESKTOP_DIR/"

echo "Installed to:"
echo "  $PLUGIN_DIR/libpowerprofiles.so"
echo "  $DESKTOP_DIR/power-profiles.desktop"
echo ""
echo "To activate, run: xfce4-panel -r"
echo "Then right-click panel > Add New Items > Power Profiles"
