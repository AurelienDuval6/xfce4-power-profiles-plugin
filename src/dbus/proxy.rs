//! zbus proxy trait for `org.freedesktop.UPower.PowerProfiles`.
//!
//! Auto-generates type-safe D-Bus methods from the trait definition.
//! Compatible with power-profiles-daemon, TLP 1.9+, and system76-power.

use std::collections::HashMap;

use zbus::proxy;
use zbus::zvariant::OwnedValue;

/// zbus proxy trait for `org.freedesktop.UPower.PowerProfiles`.
///
/// Auto-generates type-safe D-Bus methods from the trait definition.
/// Compatible with power-profiles-daemon, TLP 1.9+, and system76-power.
#[proxy(
    interface = "org.freedesktop.UPower.PowerProfiles",
    default_service = "org.freedesktop.UPower.PowerProfiles",
    default_path = "/org/freedesktop/UPower/PowerProfiles"
)]
pub trait PowerProfiles {
    /// Currently active power profile name.
    #[zbus(property)]
    fn active_profile(&self) -> zbus::Result<String>;

    /// Set the active power profile by name.
    #[zbus(property)]
    fn set_active_profile(&self, profile: &str) -> zbus::Result<()>;

    /// List of available profiles (each is a dict with "Profile" key).
    #[zbus(property)]
    fn profiles(&self) -> zbus::Result<Vec<HashMap<String, OwnedValue>>>;

    /// Performance degradation reason (empty string = no degradation).
    #[zbus(property)]
    fn performance_degraded(&self) -> zbus::Result<String>;

    /// D-Bus interface version.
    #[zbus(property)]
    fn version(&self) -> zbus::Result<String>;
}
