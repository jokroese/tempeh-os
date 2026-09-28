use std::fs;
use std::path::Path;

fn main() {
    embuild::espidf::sysenv::output();

    println!("cargo:rerun-if-changed=firmware.local.toml");
    println!("cargo:rerun-if-changed=firmware.local.example.toml");

    if let Some(config) = LocalFirmwareConfig::read("firmware.local.toml") {
        println!("cargo:rustc-env=TEMPEH_WIFI_SSID={}", config.wifi.ssid);
        println!(
            "cargo:rustc-env=TEMPEH_WIFI_PASSWORD={}",
            config.wifi.password
        );

        if let Some(tasmota) = config.tasmota {
            println!(
                "cargo:rustc-env=TEMPEH_TASMOTA_BASE_URL={}",
                tasmota.base_url
            );
        }

        if let Some(mqtt) = config.mqtt {
            println!("cargo:rustc-env=TEMPEH_MQTT_BROKER_URL={}", mqtt.broker_url);
            println!("cargo:rustc-env=TEMPEH_MQTT_DEVICE_ID={}", mqtt.device_id);
            println!(
                "cargo:rustc-env=TEMPEH_MQTT_DEVICE_NAME={}",
                mqtt.device_name
            );
            println!(
                "cargo:rustc-env=TEMPEH_MQTT_HOME_ASSISTANT_DISCOVERY={}",
                mqtt.home_assistant_discovery
            );
            if let Some(username) = mqtt.username {
                println!("cargo:rustc-env=TEMPEH_MQTT_USERNAME={username}");
            }
            if let Some(password) = mqtt.password {
                println!("cargo:rustc-env=TEMPEH_MQTT_PASSWORD={password}");
            }
        }

        println!(
            "cargo:rustc-env=TEMPEH_PROBE_BOX_AIR={}",
            config.probes.box_air
        );
        println!(
            "cargo:rustc-env=TEMPEH_PROBE_ROOM_AIR={}",
            config.probes.room_air
        );
        println!(
            "cargo:rustc-env=TEMPEH_PROBE_PRODUCT={}",
            config.probes.product
        );
    }
}

#[derive(Debug, Clone, PartialEq)]
struct LocalFirmwareConfig {
    wifi: WifiConfig,
    tasmota: Option<TasmotaConfig>,
    mqtt: Option<MqttConfig>,
    probes: ProbeConfig,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ProbeConfig {
    box_air: bool,
    room_air: bool,
    product: bool,
}

impl Default for ProbeConfig {
    fn default() -> Self {
        Self {
            box_air: true,
            room_air: false,
            product: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct WifiConfig {
    ssid: String,
    password: String,
}

#[derive(Debug, Clone, PartialEq)]
struct TasmotaConfig {
    base_url: String,
}

#[derive(Debug, Clone, PartialEq)]
struct MqttConfig {
    broker_url: String,
    username: Option<String>,
    password: Option<String>,
    device_id: String,
    device_name: String,
    home_assistant_discovery: bool,
}

impl LocalFirmwareConfig {
    fn read(path: impl AsRef<Path>) -> Option<Self> {
        let text = fs::read_to_string(path).ok()?;
        Some(Self {
            wifi: WifiConfig {
                ssid: read_toml_string(&text, "wifi", "ssid")?,
                password: read_toml_string(&text, "wifi", "password")?,
            },
            tasmota: read_toml_string(&text, "tasmota", "base_url")
                .map(|base_url| TasmotaConfig { base_url }),
            mqtt: read_toml_string(&text, "mqtt", "broker_url").map(|broker_url| MqttConfig {
                broker_url,
                username: read_toml_string(&text, "mqtt", "username"),
                password: read_toml_string(&text, "mqtt", "password"),
                device_id: read_toml_string(&text, "mqtt", "device_id")
                    .unwrap_or_else(|| "tempeh_controller".to_string()),
                device_name: read_toml_string(&text, "mqtt", "device_name")
                    .unwrap_or_else(|| "Tempeh Controller".to_string()),
                home_assistant_discovery: read_toml_bool(
                    &text,
                    "mqtt",
                    "home_assistant_discovery",
                    true,
                ),
            }),
            probes: ProbeConfig {
                box_air: read_toml_bool(&text, "probes", "box_air", true),
                room_air: read_toml_bool(&text, "probes", "room_air", false),
                product: read_toml_bool(&text, "probes", "product", false),
            },
        })
    }
}

fn read_toml_bool(text: &str, section: &str, key: &str, default: bool) -> bool {
    match read_toml_value(text, section, key) {
        Some("true") => true,
        Some("false") => false,
        _ => default,
    }
}

fn read_toml_string(text: &str, section: &str, key: &str) -> Option<String> {
    let value = read_toml_value(text, section, key)?;
    Some(value.strip_prefix('"')?.strip_suffix('"')?.to_string())
}

fn read_toml_value<'a>(text: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key} =");
    let mut current_section = None;

    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            current_section = Some(name.trim());
            continue;
        }
        if current_section != Some(section) {
            continue;
        }
        if let Some(value) = line.strip_prefix(&prefix) {
            return Some(value.trim());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_simple_toml_string() {
        let text = r#"
            [wifi]
            ssid = "tempeh-net"
            password = "secret"
        "#;

        assert_eq!(
            read_toml_string(text, "wifi", "ssid"),
            Some("tempeh-net".into())
        );
        assert_eq!(
            read_toml_string(text, "wifi", "password"),
            Some("secret".into())
        );
    }

    #[test]
    fn reads_toml_bool_with_default() {
        let text = r#"
            [probes]
            box_air = true
            room_air = false
        "#;

        assert!(read_toml_bool(text, "probes", "box_air", false));
        assert!(!read_toml_bool(text, "probes", "room_air", true));
        assert!(!read_toml_bool(text, "probes", "product", false));
    }

    #[test]
    fn reads_local_firmware_config_with_tasmota() {
        let text = r#"
            [wifi]
            ssid = "tempeh-net"
            password = "secret"

            [tasmota]
            base_url = "http://192.0.2.10"
        "#;

        assert_eq!(
            read_toml_string(text, "tasmota", "base_url"),
            Some("http://192.0.2.10".into())
        );
    }

    #[test]
    fn reads_mqtt_credentials_from_the_mqtt_section() {
        let text = r#"
            [wifi]
            password = "wifi-secret"

            [mqtt]
            broker_url = "mqtt://192.0.2.20:1883"
            username = "tempeh"
            password = "mqtt-secret"
        "#;

        assert_eq!(
            read_toml_string(text, "mqtt", "password"),
            Some("mqtt-secret".into())
        );
        assert_eq!(
            read_toml_string(text, "wifi", "password"),
            Some("wifi-secret".into())
        );
    }
}
