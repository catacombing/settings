//! Icons and symbols.

use gtk4::Image;

/// Adwaita built-in icons.
pub enum Icon {
    Locked,
    Unlocked,
    WiFiNone,
    WiFiWeak,
    WiFiOk,
    WiFiGood,
    WiFiExcellent,
}

impl Icon {
    /// Get WiFi icon from signal strength.
    pub fn wifi_from_strength(strength: u8) -> Self {
        match strength {
            0..=10 => Self::WiFiNone,
            11..=25 => Self::WiFiWeak,
            26..=60 => Self::WiFiOk,
            61..=80 => Self::WiFiGood,
            81.. => Self::WiFiExcellent,
        }
    }

    /// Get this icon as a GTK image.
    pub fn image(&self) -> Image {
        let icon_name = match self {
            Self::Locked => "changes-prevent-symbolic",
            Self::Unlocked => "changes-allow-symbolic",
            Self::WiFiNone => "network-wireless-signal-none-symbolic",
            Self::WiFiWeak => "network-wireless-signal-weak-symbolic",
            Self::WiFiOk => "network-wireless-signal-ok-symbolic",
            Self::WiFiGood => "network-wireless-signal-good-symbolic",
            Self::WiFiExcellent => "network-wireless-signal-excellent-symbolic",
        };

        Image::from_icon_name(icon_name)
    }
}
