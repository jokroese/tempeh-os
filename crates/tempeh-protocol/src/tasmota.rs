pub fn normalise_base_url(base_url: impl Into<String>) -> String {
    let mut base_url = base_url.into().trim().to_string();

    if let Some((before_query, _query)) = base_url.split_once('?') {
        base_url = before_query.to_string();
    }

    base_url = base_url.trim_end_matches('/').to_string();

    if base_url.starts_with("http://") || base_url.starts_with("https://") {
        base_url
    } else {
        format!("http://{base_url}")
    }
}

pub fn command_url(base_url: &str, command: &str) -> String {
    format!("{base_url}/cm?cmnd={command}")
}

pub fn power_command_url(base_url: &str, on: bool) -> String {
    command_url(base_url, if on { "Power%20On" } else { "Power%20Off" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_base_url_trims_and_adds_scheme() {
        assert_eq!(normalise_base_url("192.168.8.193"), "http://192.168.8.193");
        assert_eq!(
            normalise_base_url("http://192.168.8.193/"),
            "http://192.168.8.193"
        );
    }

    #[test]
    fn normalise_base_url_keeps_https_and_drops_query() {
        assert_eq!(
            normalise_base_url("  https://plug.local/?user=admin  "),
            "https://plug.local"
        );
    }

    #[test]
    fn builds_power_command_urls() {
        let base = normalise_base_url("192.168.8.193");

        assert_eq!(
            power_command_url(&base, true),
            "http://192.168.8.193/cm?cmnd=Power%20On"
        );
        assert_eq!(
            power_command_url(&base, false),
            "http://192.168.8.193/cm?cmnd=Power%20Off"
        );
    }

    #[test]
    fn builds_arbitrary_command_urls() {
        assert_eq!(
            command_url("http://192.168.8.193", "PulseTime%20120"),
            "http://192.168.8.193/cm?cmnd=PulseTime%20120"
        );
    }
}
