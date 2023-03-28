use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use glib::{clone, MainContext};
use gtk4::prelude::*;
use gtk4::{Button, Inhibit, Orientation, PasswordEntry, ScrolledWindow, Switch, Widget};
use zbus::export::futures_util::stream::StreamExt;
use zbus::zvariant::OwnedObjectPath;
use zbus::Connection;

use crate::wifi::dbus::{AccessPoint, NetworkManagerProxy};
use crate::{Navigator, SettingsPanel};

mod dbus;

/// WiFi settings.
pub struct WiFi {
    footer_buttons: [Widget; 2],
    aps_scroll: ScrolledWindow,
}

impl WiFi {
    pub fn new(navigator: Navigator) -> Self {
        // Create scrollable list for all our APs.
        let aps_scroll = ScrolledWindow::new();

        // Add footer button for re-scanning.
        let rescan_button = Button::with_label("⟳");
        rescan_button.connect_clicked(|_| {
            MainContext::default().spawn(async {
                let connection = Connection::system().await.ok()?;
                let device = dbus::wireless_device(&connection).await?;
                device.request_scan(HashMap::new()).await.ok()
            });
        });

        // Add footer button for enable/disable.
        let onoff_button = Switch::new();
        let onoff_signal = onoff_button.connect_state_set(|_, on| {
            MainContext::default().spawn(dbus::set_enabled(on));
            Inhibit(false)
        });

        let footer_buttons = [rescan_button.into(), onoff_button.clone().into()];

        // Setup NetworkManager DBus handler.
        MainContext::default().spawn_local(clone!(@strong aps_scroll => async move {
            // Attempt to connect to the system DBus.
            let connection = Connection::system().await.ok()?;

            // Get the NetworkManager device used for WiFi.
            let device = dbus::wireless_device(&connection).await?;

            // Request rescan once at startup.
            let _ = device.request_scan(HashMap::new()).await;

            // Set initial onoff button state.
            let network_manager = NetworkManagerProxy::new(&connection).await.ok()?;
            let wifi_enabled = network_manager.wireless_enabled().await.unwrap_or_default();
            onoff_button.block_signal(&onoff_signal);
            onoff_button.set_active(wifi_enabled);
            onoff_button.unblock_signal(&onoff_signal);

            tokio::join!(
                // Listen for changes in WiFi activation state.
                async {
                    let mut onoff_stream = network_manager.receive_wireless_enabled_changed().await;
                    while let Some(new_state) = onoff_stream.next().await {
                        if let Ok(new_state) = new_state.get().await {
                            onoff_button.block_signal(&onoff_signal);
                            onoff_button.set_active(new_state);
                            onoff_button.unblock_signal(&onoff_signal);
                        }
                    }
                },

                // Listen for changes in visible APs.
                async {
                    let mut ap_change_stream = device.receive_access_points_changed().await;
                    while ap_change_stream.next().await.is_some() {
                        // Update the view with our new APs.
                        let aps_box = visible_aps(navigator.clone(), &connection).await;
                        aps_scroll.set_child(aps_box.as_ref().ok());
                    }
                },

                // Listen for changes in active AP.
                async {
                    let mut active_ap_change_stream = device.receive_active_access_point_changed().await;
                    while active_ap_change_stream.next().await.is_some() {
                        // Update the view with our new APs.
                        let aps_box = visible_aps(navigator.clone(), &connection).await;
                        aps_scroll.set_child(aps_box.as_ref().ok());
                    }
                },
            );

            Some(())
        }));

        Self { aps_scroll, footer_buttons }
    }
}

impl SettingsPanel for WiFi {
    fn title(&self) -> &str {
        "WiFi"
    }

    fn widget(&self) -> Widget {
        self.aps_scroll.clone().into()
    }

    fn footer_buttons(&self) -> &[Widget] {
        &self.footer_buttons
    }
}

/// Create a box containing buttons for all visible APs.
async fn visible_aps(navigator: Navigator, connection: &Connection) -> zbus::Result<gtk4::Box> {
    let mut known_profiles = dbus::wifi_profiles(connection).await?;

    // Create new container for all the AP buttons.
    let aps_box = gtk4::Box::new(Orientation::Vertical, 0);

    // Create a button for every AP.
    let access_points = dbus::access_points(connection).await?;
    for access_point in access_points {
        // Create text identifying this AP.
        let connected = if access_point.connected { "connected - " } else { "" };
        let private = if access_point.private { "🔒" } else { "🔓" };
        let frequency = if access_point.frequency > 5000 { "5GHz" } else { "2.4GHz" };
        let ap_label = format!(
            "{connected}{} - {}% - {private} - {frequency}",
            access_point.ssid, access_point.strength,
        );

        // Get WiFi profile for this AP.
        let profile = Rc::new(known_profiles.remove(&access_point.bssid));

        // Create button for this AP.
        let navigator = navigator.clone();
        let ap_button = Button::with_label(&ap_label);
        ap_button.connect_clicked(move |_| {
            // Show dialog window.
            let dialog = WiFiDialog::new(&access_point, &profile, navigator.clone());
            navigator.show_child(navigator.clone(), &dialog.widget_box, &access_point.ssid);
        });
        aps_box.append(&ap_button);
    }

    Ok(aps_box)
}

/// WiFi AP configuration.
struct WiFiDialog {
    widget_box: gtk4::Box,
}

impl WiFiDialog {
    fn new(
        access_point: &AccessPoint,
        profile: &Option<OwnedObjectPath>,
        navigator: Navigator,
    ) -> Self {
        // Create box to hold all elements.
        let widget_box = gtk4::Box::new(Orientation::Vertical, 0);

        // Add password input if required.
        let requires_password =
            !access_point.connected && access_point.private && !profile.is_some();
        let password_input = requires_password.then(|| {
            let password_input = PasswordEntry::new();
            password_input.set_show_peek_icon(true);
            widget_box.append(&password_input);
            password_input
        });

        // Add "Forget" button if network is known.
        let profile = Arc::new(profile.to_owned());
        if let Some(profile) = &*profile {
            // Create and add button.
            let forget_button = Button::with_label("Forget");
            widget_box.append(&forget_button);

            // Add forget button handler.
            let forget_navigator = navigator.clone();
            let profile = profile.clone();
            forget_button.connect_clicked(move |_| {
                MainContext::default().spawn(dbus::forget(profile.clone()));
                forget_navigator.pop();
            });
        }

        // Determine confirm button label.
        let confirm_label = if access_point.connected { "Disconnect" } else { "Connect" };

        // Create and add confirm button.
        let confirm_button = Button::with_label(confirm_label);
        widget_box.append(&confirm_button);

        // Add confirm button handler.
        let access_point = Arc::new(access_point.clone());
        confirm_button.connect_clicked(clone!(@strong password_input => move |_| {
            let password = password_input.as_ref().map(|input| input.text().as_str().to_owned());

            let access_point = access_point.clone();
            let profile = profile.clone();

            // Perform requested connection change.
            MainContext::default().spawn(async move {
                if access_point.connected {
                    let _ = dbus::disconnect(&access_point.ssid).await;
                } else if let Some(profile) = profile.as_ref() {
                    let _ = dbus::reconnect(&access_point, profile.as_ref().to_owned()).await;
                } else {
                    let _ = dbus::connect(&access_point, password).await;
                }
            });

            // Navigate back to the parent.
            navigator.pop();
        }));

        Self { widget_box }
    }
}
