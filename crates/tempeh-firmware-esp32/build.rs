use std::fs;
use std::path::Path;

fn main() {
    embuild::espidf::sysenv::output();

    println!("cargo:rerun-if-changed=firmware.local.toml");
    println!("cargo:rerun-if-changed=firmware.local.example.toml");

    if let Some(config) = LocalFirmwareConfig::read("firmware.local.toml") {
        println!("cargo:rustc-env=TEMPEH_WIFI_SSID={}", config.wifi_ssid);
        println!(
            "cargo:rustc-env=TEMPEH_WIFI_PASSWORD={}",
            config.wifi_password
        );
    }
}

#[derive(Debug, Clone, PartialEq)]
struct LocalFirmwareConfig {
    wifi_ssid: String,
    wifi_password: String,
}

impl LocalFirmwareConfig {
    fn read(path: impl AsRef<Path>) -> Option<Self> {
        let text = fs::read_to_string(path).ok()?;
        Some(Self {
            wifi_ssid: read_toml_string(&text, "ssid")?,
            wifi_password: read_toml_string(&text, "password")?,
        })
    }
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
}
