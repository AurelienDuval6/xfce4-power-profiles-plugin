# Setup

## Prerequisites

| Dependency | Purpose |
|---|---|
| Rust (stable, edition 2021) | Compiler |
| GCC | Linking the shared library |
| pkg-config | Locating GTK3 and xfce4-panel headers |
| GTK 3.24+ development files | UI toolkit |
| xfce4-panel development files | Panel plugin API |
| libxfce4util development files | XFCE utility library |

### Arch Linux

```bash
sudo pacman -S rust gcc pkg-config gtk3 xfce4-panel
```

### Debian

```bash
sudo apt install rustc gcc pkg-config libgtk-3-dev libxfce4panel-2.0-dev libxfce4ui-2-dev libxfce4util-dev
```

### Fedora

```bash
sudo dnf install rust gcc pkg-config gtk3-devel xfce4-panel-devel libxfce4ui-devel libxfce4util-devel
```

## Building

```bash
bash build.sh
```

This runs three stages:
1. `cargo build --release` — compiles the Rust static library
2. `gcc -c` — compiles the C shim (`plugin.c`)
3. `gcc -shared` — links everything into `libpowerprofiles.so`

## Installing

### Prebuilt packages (Debian)

Prebuilt `.deb` packages are built on every release and attached to the matching
GitHub Release. Download the package for your distro/architecture and install it:

```bash
sudo apt install ./xfce4-power-profiles-plugin_<version>_<arch>.deb
```

Supported targets (built in CI):

- Debian 12 (bookworm)
- Debian 13 (trixie)

After installing, restart the panel and add the plugin:

```bash
xfce4-panel -r
```

Right-click the panel → **Add New Items** → **Power Profiles**.

The `.deb` installs:

- `/usr/lib/xfce4/panel/plugins/libpowerprofiles.so`
- `/usr/share/xfce4/panel/plugins/power-profiles.desktop`

### Local (user)

```bash
bash install.sh
xfce4-panel -r
```

Installs to:
- `~/.local/lib/xfce4/panel/plugins/libpowerprofiles.so`
- `~/.local/share/xfce4/panel/plugins/power-profiles.desktop`

### System-wide

```bash
sudo install -Dm755 libpowerprofiles.so /usr/lib/xfce4/panel/plugins/libpowerprofiles.so
sudo install -Dm644 power-profiles.desktop /usr/share/xfce4/panel/plugins/power-profiles.desktop
xfce4-panel -r
```

## Generating Documentation

```bash
cargo doc --no-deps
```

API docs are generated at `target/doc/powerprofiles/index.html`. All source files have `//!` module-level docs and `///` doc comments on public items.

## Adding to the Panel

After installing, right-click the panel → **Add New Items** → **Power Profiles**.

## Configuration

The plugin has no configuration files. Profile selection is managed via the panel button and popup slider. Changes are written to the D-Bus backend immediately.

## Backend Requirements

At least one compatible power management backend must be running:

### power-profiles-daemon

```bash
systemctl status power-profiles-daemon
```

This is the default on most systemd-based distributions.

### TLP

TLP 1.9+ supports the `power-profiles-daemon` D-Bus interface when `TLP_PSM_ENABLE_DBUS=1` is set in `/etc/tlp.conf`.

```bash
sudo systemctl enable --now tlp
```

### system76-power

Used on System76 hardware running Pop!_OS:

```bash
sudo systemctl enable --now system76-power
```
