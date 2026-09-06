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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TasmotaReply {
    pub power: Option<PowerState>,
}

impl TasmotaReply {
    pub fn confirms(&self, requested_on: bool) -> bool {
        match self.power {
            Some(PowerState::On) => requested_on,
            Some(PowerState::Off) => !requested_on,
            None => false,
        }
    }
}

pub fn parse_power_response(body: &str) -> TasmotaReply {
    TasmotaReply {
        power: find_power_value(body),
    }
}

fn find_power_value(body: &str) -> Option<PowerState> {
    let mut rest = body;

    while let Some(offset) = rest.find("\"POWER") {
        let after_key = &rest[offset + 6..];
        let Some(colon) = after_key.find(':') else {
            return None;
        };

        let key_tail = after_key[..colon].trim_end_matches(|c: char| c.is_whitespace());
        if key_tail.ends_with('"')
            && key_tail[..key_tail.len() - 1]
                .chars()
                .all(|c| c.is_ascii_digit())
        {
            if let Some(state) = read_power_state(&after_key[colon + 1..]) {
                return Some(state);
            }
        }

        rest = &after_key[colon..];
    }

    None
}

fn read_power_state(text: &str) -> Option<PowerState> {
    let value = text.trim_start();
    let value = value.strip_prefix('"')?;
    let end = value.find('"')?;

    match &value[..end] {
        "ON" => Some(PowerState::On),
        "OFF" => Some(PowerState::Off),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POWER_ON: &str = r#"{"POWER":"ON"}"#;
    const POWER_OFF: &str = r#"{"POWER":"OFF"}"#;
    const POWER1_ON: &str = r#"{"POWER1":"ON"}"#;
    const PULSETIME: &str =
        r#"{"PulseTime":{"Set":120,"Remaining":0,"Timer1":0,"Timer2":0,"Timer3":0,"Timer4":0}}"#;
    const POWER_ON_STATE: &str = r#"{"PowerOnState":0}"#;
    const UNKNOWN_COMMAND: &str = r#"{"Command":"Unknown"}"#;
    const TRUNCATED: &str = r#"{"POWER":"O"#;

    #[test]
    fn parses_power_on_and_off_replies() {
        assert_eq!(
            parse_power_response(POWER_ON),
            TasmotaReply {
                power: Some(PowerState::On)
            }
        );
        assert_eq!(
            parse_power_response(POWER_OFF),
            TasmotaReply {
                power: Some(PowerState::Off)
            }
        );
    }

    #[test]
    fn parses_an_indexed_power_reply() {
        assert_eq!(
            parse_power_response(POWER1_ON),
            TasmotaReply {
                power: Some(PowerState::On)
            }
        );
    }

    #[test]
    fn tolerates_extra_fields() {
        let body = r#"{"Time":"2026-09-06T10:00:00","Uptime":"0T00:01:02","POWER":"ON","Wifi":{"RSSI":72}}"#;

        assert_eq!(
            parse_power_response(body),
            TasmotaReply {
                power: Some(PowerState::On)
            }
        );
    }

    #[test]
    fn reports_no_power_state_for_configuration_replies() {
        for body in [PULSETIME, POWER_ON_STATE, UNKNOWN_COMMAND, TRUNCATED, ""] {
            assert_eq!(parse_power_response(body), TasmotaReply { power: None });
        }
    }

    #[test]
    fn a_missing_power_state_never_confirms() {
        let reply = parse_power_response(UNKNOWN_COMMAND);

        assert!(!reply.confirms(true));
        assert!(!reply.confirms(false));
    }

    #[test]
    fn a_contradictory_power_state_never_confirms() {
        assert!(!parse_power_response(POWER_OFF).confirms(true));
        assert!(!parse_power_response(POWER_ON).confirms(false));
    }

    #[test]
    fn a_matching_power_state_confirms() {
        assert!(parse_power_response(POWER_ON).confirms(true));
        assert!(parse_power_response(POWER_OFF).confirms(false));
    }

    #[test]
    fn ignores_a_power_key_without_a_string_value() {
        assert_eq!(
            parse_power_response(r#"{"POWERUP":"ON","POWER":"OFF"}"#),
            TasmotaReply {
                power: Some(PowerState::Off)
            }
        );
    }

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
