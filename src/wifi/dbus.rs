use std::cmp::Ordering;
use std::collections::HashMap;

use byteorder::LE;
use zbus::zvariant::{
    self, Array, EncodingContext, ObjectPath, OwnedObjectPath, OwnedValue, Str, Type, Value,
};
use zbus::{dbus_proxy, Connection};

/// NetworkManager access point.
#[derive(Clone, Debug)]
pub struct AccessPoint {
    /// AP hardware address.
    pub bssid: String,

    /// Access point name.
    pub ssid: String,

    /// Signal strength in percent.
    pub strength: u8,

    /// Requires password authentication.
    pub private: bool,

    /// WiFi frequency in MHz.
    pub frequency: u32,

    /// Access point is currently active.
    pub connected: bool,

    /// DBus access point object path.
    pub path: OwnedObjectPath,
}

impl AccessPoint {
    pub async fn from_nm_ap(
        connection: &Connection,
        path: OwnedObjectPath,
        active_bssid: Option<&str>,
    ) -> zbus::Result<Self> {
        let ap = AccessPointProxy::builder(connection).path(&path)?.build().await?;

        let ssid_bytes = ap.ssid().await?;
        let ssid = String::from_utf8(ssid_bytes).map_err(|_| zbus::Error::InvalidField)?;
        let private = ap.flags().await? != APFlags::None;
        let strength = ap.strength().await?;
        let frequency = ap.frequency().await?;
        let bssid = ap.hw_address().await?;
        let connected = active_bssid.map_or(false, |active| bssid == active);

        Ok(Self { ssid, strength, private, frequency, bssid, connected, path })
    }
}

/// Set NetworkManager WiFi state.
pub async fn set_enabled(enabled: bool) -> zbus::Result<()> {
    let connection = Connection::system().await?;
    let network_manager = NetworkManagerProxy::new(&connection).await?;
    network_manager.set_wireless_enabled(enabled).await
}

/// Get all APs.
pub async fn access_points(connection: &Connection) -> zbus::Result<Vec<AccessPoint>> {
    // Get the WiFi device.
    let device = match wireless_device(connection).await {
        Some(device) => device,
        None => return Ok(Vec::new()),
    };

    // Get the active access point.
    let active_ap = match device.active_access_point().await {
        // Filter out fallback AP `/`.
        Ok(path) if path.len() != 1 => AccessPoint::from_nm_ap(connection, path, None).await.ok(),
        _ => None,
    };
    let active_bssid = active_ap.as_ref().map(|ap| ap.bssid.as_str());

    // Get all access points.
    let aps = device.access_points().await?;

    // Collect required data from NetworkManager access points.
    let mut access_points = Vec::new();
    for ap in aps {
        let access_point = AccessPoint::from_nm_ap(connection, ap, active_bssid).await;
        if let Ok(access_point) = access_point {
            access_points.push(access_point);
        }
    }

    // Sort by signal strength.
    access_points.sort_unstable_by(|a, b| match b.connected.cmp(&a.connected) {
        Ordering::Equal => b.strength.cmp(&a.strength),
        ordering => ordering,
    });

    Ok(access_points)
}

/// Get the wireless device.
pub async fn wireless_device(connection: &Connection) -> Option<WirelessDeviceProxy> {
    // Get network manager interface.
    let network_manager = NetworkManagerProxy::new(connection).await.ok()?;

    // Get realized network devices.
    let device_paths = network_manager.get_devices().await.ok()?;

    // Return the first wifi network device.
    for device_path in device_paths {
        let wireless_device = wireless_device_from_path(connection, device_path).await;
        if wireless_device.is_some() {
            return wireless_device;
        }
    }

    None
}

/// Try and convert a NetworkManager device path to a wireless device.
async fn wireless_device_from_path(
    connection: &Connection,
    device_path: OwnedObjectPath,
) -> Option<WirelessDeviceProxy> {
    // Resolve as generic device first.
    let device = DeviceProxy::builder(connection).path(&device_path).ok()?.build().await.ok()?;

    // Skip devices with incorrect type.
    if !matches!(device.device_type().await, Ok(DeviceType::Wifi)) {
        return None;
    }

    // Try ta resolve as wireless device.
    WirelessDeviceProxy::builder(connection).path(device_path).ok()?.build().await.ok()
}

/// Connect to an AP with a new profile.
pub async fn connect(access_point: &AccessPoint, password: Option<String>) -> zbus::Result<()> {
    let connection = Connection::system().await?;

    // Get path for our wireless device.
    let device = match wireless_device(&connection).await {
        Some(device) => device,
        None => return Ok(()),
    };
    let device_path = device.path().to_owned();

    // Get AP object path.
    let ap_path = access_point.path.as_ref();

    let mut settings = HashMap::new();

    // Add connection settings.
    let mut connection_settings = HashMap::new();
    connection_settings.insert("id", Value::Str(Str::from(&access_point.ssid)));
    connection_settings.insert("type", Value::Str(Str::from("802-11-wireless")));
    settings.insert("connection", connection_settings);

    // Convert SSID to byte array.
    let context = EncodingContext::<LE>::new_dbus(0);
    let ssid_sliced = zvariant::to_bytes(context, &access_point.ssid)?;

    // Add WiFi settings.
    let mut wifi_settings = HashMap::new();
    wifi_settings.insert("mode", Value::Str(Str::from("infrastructure")));
    wifi_settings.insert("ssid", Value::Array(Array::from(ssid_sliced)));

    // Add password settings.
    if let Some(password) = password {
        let mut security_settings = HashMap::new();
        security_settings.insert("auth-alg", Value::Str(Str::from("open")));
        security_settings.insert("psk", Value::Str(Str::from(password)));
        security_settings.insert("key-mgmt", Value::Str(Str::from("wpa-psk")));
        settings.insert("802-11-wireless-security", security_settings);
    }

    // Create and activate the profile.
    let network_manager = NetworkManagerProxy::new(&connection).await?;
    network_manager.add_and_activate_connection(settings, device_path, ap_path).await?;

    Ok(())
}

/// Reconnect to a known AP.
pub async fn reconnect(
    access_point: &AccessPoint,
    profile: ObjectPath<'static>,
) -> zbus::Result<()> {
    let connection = Connection::system().await?;

    // Get path for our wireless device.
    let device = match wireless_device(&connection).await {
        Some(device) => device,
        None => return Ok(()),
    };
    let device_path = device.path().to_owned();

    // Get AP object path.
    let ap_path = access_point.path.as_ref();

    let network_manager = NetworkManagerProxy::new(&connection).await?;
    network_manager.activate_connection(profile, device_path, ap_path).await?;

    Ok(())
}

/// Disconnect from an active connection.
pub async fn disconnect(ssid: &str) -> zbus::Result<()> {
    let connection = Connection::system().await?;
    let network_manager = NetworkManagerProxy::new(&connection).await?;

    let active_connections = network_manager.active_connections().await?;
    for path in active_connections {
        let active_connection =
            ActiveConnectionProxy::builder(&connection).path(&path)?.build().await?;
        let id = active_connection.id().await?;
        if id == ssid {
            network_manager.deactivate_connection(path.as_ref()).await?;
            break;
        }
    }

    Ok(())
}

/// Delete a WiFi profile.
pub async fn forget(profile_path: OwnedObjectPath) -> zbus::Result<()> {
    let connection = Connection::system().await?;
    let profile = ConnectionProxy::builder(&connection).path(profile_path)?.build().await?;
    profile.delete().await
}

/// Get known WiFi connection settings by BSSID.
pub async fn wifi_profiles(
    connection: &Connection,
) -> zbus::Result<HashMap<String, OwnedObjectPath>> {
    // Get network profiles.
    let settings = SettingsProxy::new(connection).await?;
    let network_profiles = settings.list_connections().await?;

    // Get BSSIDs for all known profiles.
    let mut profiles = HashMap::new();
    for profile_path in network_profiles {
        for bssid in wifi_bssids(connection, &profile_path).await.unwrap_or_default() {
            profiles.insert(bssid, profile_path.clone());
        }
    }

    Ok(profiles)
}

/// Get BSSIDs for a WiFi connection setting.
async fn wifi_bssids(
    connection: &Connection,
    profile_path: &OwnedObjectPath,
) -> Option<Vec<String>> {
    // Extract BSSIDs from settings.
    let profile =
        ConnectionProxy::builder(connection).path(profile_path).ok()?.build().await.ok()?;
    let settings = profile.get_settings().await.ok()?;
    let wifi_settings = settings.get("802-11-wireless")?;
    let bssids_setting = wifi_settings.get("seen-bssids")?;

    // Convert BSSID array to Rust array.
    let bssid_values = match &**bssids_setting {
        Value::Array(array) => array.get(),
        _ => return None,
    };

    // Convert BSSID value string to Rust string.
    let bssids = bssid_values
        .iter()
        .filter_map(|value| match value {
            Value::Str(bssid) => Some(bssid.as_str().to_owned()),
            _ => None,
        })
        .collect();

    Some(bssids)
}

#[dbus_proxy(assume_defaults = true)]
trait NetworkManager {
    /// Get the list of realized network devices.
    fn get_devices(&self) -> zbus::Result<Vec<OwnedObjectPath>>;

    /// Activate a connection using the supplied device.
    fn activate_connection(
        &self,
        connection: ObjectPath<'_>,
        device: ObjectPath<'_>,
        specific_object: ObjectPath<'_>,
    ) -> zbus::Result<OwnedObjectPath>;

    /// Adds a new connection using the given details (if any) as a template
    /// (automatically filling in missing settings with the capabilities of the
    /// given device and specific object), then activate the new connection.
    /// Cannot be used for VPN connections at this time.
    fn add_and_activate_connection(
        &self,
        connection: HashMap<&str, HashMap<&str, Value<'_>>>,
        device: ObjectPath<'_>,
        specific_object: ObjectPath<'_>,
    ) -> zbus::Result<(OwnedObjectPath, OwnedObjectPath)>;

    /// Deactivate an active connection.
    fn deactivate_connection(&self, connection: ObjectPath<'_>) -> zbus::Result<()>;

    /// Control whether overall networking is enabled or disabled. When
    /// disabled, all interfaces that NM manages are deactivated. When enabled,
    /// all managed interfaces are re-enabled and available to be activated.
    /// This command should be used by clients that provide to users the ability
    /// to enable/disable all networking.
    fn enable(&self, enable: bool) -> zbus::Result<()>;

    /// Indicates if wireless is currently enabled or not.
    #[dbus_proxy(property)]
    fn wireless_enabled(&self) -> zbus::Result<bool>;

    /// Set if wireless is currently enabled or not.
    #[dbus_proxy(property)]
    fn set_wireless_enabled(&self, enabled: bool) -> zbus::Result<()>;

    /// List of active connection object paths.
    #[dbus_proxy(property)]
    fn active_connections(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
}

#[dbus_proxy(
    interface = "org.freedesktop.NetworkManager.Device",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager/Device"
)]
trait Device {
    /// Disconnects a device and prevents the device from automatically
    /// activating further connections without user intervention.
    fn disconnect(&self) -> zbus::Result<()>;

    /// The general type of the network device; ie Ethernet, Wi-Fi, etc.
    #[dbus_proxy(property)]
    fn device_type(&self) -> zbus::Result<DeviceType>;
}

#[dbus_proxy(
    interface = "org.freedesktop.NetworkManager.Device.Wireless",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager/Device/Wireless"
)]
trait WirelessDevice {
    /// Request the device to scan. To know when the scan is finished, use the
    /// "PropertiesChanged" signal from "org.freedesktop.DBus.Properties" to
    /// listen to changes to the "LastScan" property.
    fn request_scan(&self, options: HashMap<String, OwnedValue>) -> zbus::Result<()>;

    /// List of object paths of access point visible to this wireless device.
    #[dbus_proxy(property)]
    fn access_points(&self) -> zbus::Result<Vec<OwnedObjectPath>>;

    /// Object path of the access point currently used by the wireless device.
    #[dbus_proxy(property)]
    fn active_access_point(&self) -> zbus::Result<OwnedObjectPath>;
}

#[dbus_proxy(
    interface = "org.freedesktop.NetworkManager.AccessPoint",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager/AccessPoint"
)]
trait AccessPoint {
    /// Flags describing the capabilities of the access point.
    #[dbus_proxy(property)]
    fn flags(&self) -> zbus::Result<APFlags>;

    /// The Service Set Identifier identifying the access point.
    #[dbus_proxy(property)]
    fn ssid(&self) -> zbus::Result<Vec<u8>>;

    /// The radio channel frequency in use by the access point, in MHz.
    #[dbus_proxy(property)]
    fn frequency(&self) -> zbus::Result<u32>;

    /// The hardware address (BSSID) of the access point.
    #[dbus_proxy(property)]
    fn hw_address(&self) -> zbus::Result<String>;

    /// The current signal quality of the access point, in percent.
    #[dbus_proxy(property)]
    fn strength(&self) -> zbus::Result<u8>;
}

#[dbus_proxy(
    interface = "org.freedesktop.NetworkManager.Settings",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager/Settings"
)]
trait Settings {
    /// List the saved network connections known to NetworkManager.
    fn list_connections(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
}

#[dbus_proxy(
    interface = "org.freedesktop.NetworkManager.Settings.Connection",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager/Settings/Connection"
)]
trait Connection {
    /// Delete the connection.
    fn delete(&self) -> zbus::Result<()>;

    /// Get the settings maps describing this network configuration. This will
    /// never include any secrets required for connection to the network, as
    /// those are often protected. Secrets must be requested separately using
    /// the GetSecrets() call.
    fn get_settings(&self) -> zbus::Result<HashMap<String, HashMap<String, OwnedValue>>>;

    /// Get the secrets belonging to this network configuration. Only secrets
    /// from persistent storage or a Secret Agent running in the requestor's
    /// session will be returned. The user will never be prompted for secrets as
    /// a result of this request.
    fn get_secrets(
        &self,
        setting_name: &str,
    ) -> zbus::Result<HashMap<String, HashMap<String, OwnedValue>>>;
}

#[dbus_proxy(
    interface = "org.freedesktop.NetworkManager.Connection.Active",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager/ActiveConnection"
)]
trait ActiveConnection {
    /// The ID of the connection, provided as a convenience so that clients do
    /// not have to retrieve all connection details.
    #[dbus_proxy(property)]
    fn id(&self) -> zbus::Result<String>;
}

/// NMDeviceType values indicate the type of hardware represented by a device
/// object.
#[derive(Type, OwnedValue, PartialEq, Debug)]
#[repr(u32)]
pub enum DeviceType {
    Wifi = 2,
    Modem = 8,
}

/// 802.11 access point flags.
#[derive(Type, OwnedValue, PartialEq, Debug)]
#[repr(u32)]
pub enum APFlags {
    None = 0,
    Privacy = 1,
    Wps = 2,
    WpsPbc = 4,
    WpsPin = 8,
}
