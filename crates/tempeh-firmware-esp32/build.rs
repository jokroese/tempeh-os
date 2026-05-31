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
            product: true,
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

impl LocalFirmwareConfig {
    fn read(path: impl AsRef<Path>) -> Option<Self> {
        let text = fs::read_to_string(path).ok()?;
        Some(Self {
            wifi: WifiConfig {
                ssid: read_toml_string(&text, "ssid")?,
                password: read_toml_string(&text, "password")?,
            },
            tasmota: read_toml_string(&text, "base_url").map(|base_url| TasmotaConfig { base_url }),
            probes: ProbeConfig {
                box_air: read_toml_bool(&text, "box_air", true),
                room_air: read_toml_bool(&text, "room_air", false),
                product: read_toml_bool(&text, "product", true),
            },
        })
    }
}

fn read_toml_bool(text: &str, key: &str, default: bool) -> bool {
    let prefix = format!("{key} =");

    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| {
            let value = line.strip_prefix(&prefix)?.trim();
            Some(match value {
                "true" => true,
                "false" => false,
                _ => default,
            })
        })
        .unwrap_or(default)
}

fn read_toml_string(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} =");

    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| {
            let value = line.strip_prefix(&prefix)?.trim();
            let value = value.strip_prefix('"')?.strip_suffix('"')?;
            Some(value.to_string())
        })
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

        assert_eq!(read_toml_string(text, "ssid"), Some("tempeh-net".into()));
        assert_eq!(read_toml_string(text, "password"), Some("secret".into()));
    }

    #[test]
    fn reads_toml_bool_with_default() {
        let text = r#"
            [probes]
            box_air = true
            room_air = false
        "#;

        assert!(read_toml_bool(text, "box_air", false));
        assert!(!read_toml_bool(text, "room_air", true));
        assert!(read_toml_bool(text, "product", true));
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
            read_toml_string(text, "base_url"),
            Some("http://192.0.2.10".into())
        );
    }
}
