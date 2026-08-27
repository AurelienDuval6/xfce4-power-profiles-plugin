//! D-Bus communication with `org.freedesktop.UPower.PowerProfiles`.
//!
//! Runs on a dedicated background thread with its own tokio runtime.
//! Events are sent to the GTK main loop via [`std::sync::mpsc`].
//!
//! # Signal Monitoring
//!
//! Subscribes to `PropertiesChanged` signals from the `PowerProfiles` service
//! and forwards changes as [`DbusEvent`] variants.

pub mod proxy;

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Duration;

use futures_util::stream::StreamExt;
use proxy::PowerProfilesProxy;
use zbus::match_rule::MatchRule;
use zbus::message::Type;
use zbus::zvariant::Str;

/// D-Bus event sent from the background thread to the GTK main loop.
pub enum DbusEvent {
    /// Active profile name changed (via user or external).
    ActiveProfileChanged(String),
    /// Available profiles list changed (e.g. backend restarted).
    ProfilesChanged(Vec<String>),
    /// Performance degradation warning text (empty = no degradation).
    PerformanceDegraded(String),
    /// D-Bus unavailable — carries an error message for the tooltip.
    Unavailable(String),
}

/// Extracts profile name strings from the `Profiles` D-Bus property.
///
/// Each entry is a dict with a `"Profile"` key containing the name string.
fn extract_profile_names(profiles: &[HashMap<String, zbus::zvariant::OwnedValue>]) -> Vec<String> {
    profiles
        .iter()
        .filter_map(|p| {
            p.get("Profile")
                .and_then(|v| v.downcast_ref::<Str>().ok())
                .map(|s| s.as_str().to_owned())
        })
        .collect()
}

/// Manages all D-Bus communication on a dedicated background thread.
///
/// Spawns a background thread with its own tokio runtime for D-Bus operations.
/// Events are sent to the GTK main loop via `mpsc::Sender<DbusEvent>`.
pub struct DbusManager {
    _handle: Option<std::thread::JoinHandle<()>>,
}

impl DbusManager {
    /// Creates a new D-Bus manager. Spawns a background thread immediately.
    ///
    /// # Panics
    /// Panics if the tokio runtime fails to start.
    #[must_use]
    pub fn new(tx: mpsc::Sender<DbusEvent>) -> Self {
        let handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime");

            rt.block_on(async move {
                Self::run_dbus_loop(tx).await;
            });
        });

        Self {
            _handle: Some(handle),
        }
    }

    /// Sets the active profile via D-Bus. Spawns a short-lived thread with its
    /// own single-threaded tokio runtime to avoid blocking the main loop.
    ///
    /// # Panics
    /// Panics if the tokio runtime fails to start.
    pub fn set_profile(&self, profile: &str) {
        let profile = profile.to_owned();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime");
            rt.block_on(async move {
                if let Err(e) = Self::set_profile_async(&profile).await {
                    eprintln!("Failed to set profile '{profile}': {e}");
                }
            });
        });
    }

    async fn set_profile_async(profile: &str) -> zbus::Result<()> {
        let connection = zbus::Connection::system().await?;
        let proxy = PowerProfilesProxy::new(&connection).await?;
        proxy.set_active_profile(profile).await?;
        Ok(())
    }

    /// Main D-Bus event loop. Runs on the background thread's tokio runtime.
    ///
    /// 1. Connects to the system bus
    /// 2. Reads initial `ActiveProfile` and `Profiles`
    /// 3. Subscribes to `PropertiesChanged` signals
    /// 4. Forwards changes to the GTK main loop via the channel
    async fn run_dbus_loop(tx: mpsc::Sender<DbusEvent>) {
        let connection = match zbus::Connection::system().await {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(DbusEvent::Unavailable(format!(
                    "Cannot connect to system bus: {e}"
                )));
                return;
            }
        };

        let proxy = match PowerProfilesProxy::new(&connection).await {
            Ok(p) => p,
            Err(e) => {
                let _ = tx.send(DbusEvent::Unavailable(format!("Cannot create proxy: {e}")));
                return;
            }
        };

        match proxy.profiles().await {
            Ok(profiles) => {
                let _ = tx.send(DbusEvent::ProfilesChanged(extract_profile_names(&profiles)));
            }
            Err(e) => {
                let _ = tx.send(DbusEvent::Unavailable(format!("Cannot read profiles: {e}")));
                return;
            }
        }

        match proxy.active_profile().await {
            Ok(profile) => {
                let _ = tx.send(DbusEvent::ActiveProfileChanged(profile));
            }
            Err(e) => {
                let _ = tx.send(DbusEvent::Unavailable(format!(
                    "Cannot read active profile: {e}"
                )));
            }
        }

        // Subscribe to PropertiesChanged signals from the PowerProfiles service.
        // Uses `.sender()` (not `.destination()`) because signals are broadcasts.
        let rule = match MatchRule::builder()
            .msg_type(Type::Signal)
            .sender("org.freedesktop.UPower.PowerProfiles")
            .and_then(|b| b.interface("org.freedesktop.DBus.Properties"))
            .and_then(|b| b.member("PropertiesChanged"))
            .map(zbus::match_rule::Builder::build)
        {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(DbusEvent::Unavailable(format!(
                    "Cannot build match rule: {e}"
                )));
                return;
            }
        };

        let mut stream = match zbus::MessageStream::for_match_rule(rule, &connection, None).await {
            Ok(s) => s,
            Err(e) => {
                let _ = tx.send(DbusEvent::Unavailable(format!(
                    "Cannot watch properties: {e}"
                )));
                return;
            }
        };

        // Event loop: receive D-Bus signals and forward changes to GTK.
        // Uses tokio::select! with a 500ms timeout to keep the loop responsive.
        loop {
            tokio::select! {
                msg = stream.next() => {
                    if let Some(Ok(msg)) = msg {
                        // PropertiesChanged signature: (STRING, DICT, ARRAY)
                        let result: Result<(String, HashMap<String, zbus::zvariant::OwnedValue>, Vec<String>), _> =
                            msg.body().deserialize();
                        if let Ok((_interface, changed, _invalidated)) = result {
                            if let Some(active) = changed.get("ActiveProfile") {
                                if let Ok(val) = active.downcast_ref::<Str>() {
                                    let _ = tx.send(DbusEvent::ActiveProfileChanged(val.as_str().to_owned()));
                                }
                            }
                            if let Some(perf) = changed.get("PerformanceDegraded") {
                                if let Ok(val) = perf.downcast_ref::<Str>() {
                                    let _ = tx.send(DbusEvent::PerformanceDegraded(val.as_str().to_owned()));
                                }
                            }
                            if changed.contains_key("Profiles") {
                                if let Ok(profiles) = proxy.profiles().await {
                                    let _ = tx.send(DbusEvent::ProfilesChanged(extract_profile_names(&profiles)));
                                }
                            }
                        }
                    } else {
                        let _ = tx.send(DbusEvent::Unavailable("D-Bus stream ended".to_owned()));
                        break;
                    }
                }
                () = tokio::time::sleep(Duration::from_millis(500)) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::extract_profile_names;
    use std::collections::HashMap;
    use zbus::zvariant::{OwnedValue, Str};

    fn profile_dict(name: &str) -> HashMap<String, OwnedValue> {
        HashMap::from([("Profile".to_owned(), OwnedValue::from(Str::from(name)))])
    }

    #[test]
    fn extracts_profile_names_from_valid_dicts() {
        let profiles = [profile_dict("power-saver"), profile_dict("balanced")];
        assert_eq!(
            extract_profile_names(&profiles),
            ["power-saver", "balanced"]
        );
    }

    #[test]
    fn returns_empty_vec_for_empty_input() {
        let profiles: Vec<HashMap<String, OwnedValue>> = Vec::new();
        assert!(extract_profile_names(&profiles).is_empty());
    }

    #[test]
    fn skips_dicts_without_a_profile_key() {
        let mut without_key = profile_dict("balanced");
        without_key.remove("Profile");
        let profiles = [without_key, profile_dict("performance")];
        assert_eq!(extract_profile_names(&profiles), ["performance"]);
    }

    #[test]
    fn skips_profile_entries_that_are_not_strings() {
        let mut wrong_type = HashMap::new();
        wrong_type.insert("Profile".to_owned(), OwnedValue::from(42u32));
        let profiles = [wrong_type, profile_dict("performance")];
        assert_eq!(extract_profile_names(&profiles), ["performance"]);
    }

    #[test]
    fn returns_names_in_input_order() {
        let profiles = [
            profile_dict("performance"),
            profile_dict("balanced"),
            profile_dict("power-saver"),
        ];
        assert_eq!(
            extract_profile_names(&profiles),
            ["performance", "balanced", "power-saver"]
        );
    }
}
