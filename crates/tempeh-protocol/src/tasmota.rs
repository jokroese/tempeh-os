use serde_json::{Map, Value};

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

pub fn pulse_time_command_url(base_url: &str, value: u32) -> String {
    command_url(base_url, &format!("PulseTime%20{value}"))
}

pub fn power_on_state_command_url(base_url: &str, value: u8) -> String {
    command_url(base_url, &format!("PowerOnState%20{value}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TasmotaResponseError {
    InvalidJson,
    ExpectedObject,
    MissingPowerState,
    InvalidPowerState,
    ConflictingPowerStates,
    MissingPulseTime,
    InvalidPulseTime,
    MissingPowerOnState,
    InvalidPowerOnState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TasmotaReply {
    pub power: PowerState,
}

impl TasmotaReply {
    pub fn confirms(&self, requested_on: bool) -> bool {
        match self.power {
            PowerState::On => requested_on,
            PowerState::Off => !requested_on,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PulseTimeReply {
    pub set: u32,
}

impl PulseTimeReply {
    pub fn confirms(&self, requested_value: u32) -> bool {
        self.set == requested_value
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerOnStateReply {
    pub state: u8,
}

impl PowerOnStateReply {
    pub fn confirms(&self, requested_value: u8) -> bool {
        self.state == requested_value
    }
}

pub fn parse_power_response(body: &str) -> Result<TasmotaReply, TasmotaResponseError> {
    let object = parse_object(body)?;
    let mut power = None;

    for (key, value) in &object {
        if !is_indexed_key(key, "POWER") {
            continue;
        }

        let state = match value.as_str() {
            Some("ON") => PowerState::On,
            Some("OFF") => PowerState::Off,
            _ => return Err(TasmotaResponseError::InvalidPowerState),
        };

        if let Some(previous) = power
            && previous != state
        {
            return Err(TasmotaResponseError::ConflictingPowerStates);
        }

        power = Some(state);
    }

    power
        .map(|power| TasmotaReply { power })
        .ok_or(TasmotaResponseError::MissingPowerState)
}

pub fn parse_pulse_time_response(body: &str) -> Result<PulseTimeReply, TasmotaResponseError> {
    let object = parse_object(body)?;
    let mut values = object
        .iter()
        .filter(|(key, _)| is_indexed_key(key, "PulseTime"))
        .map(|(_, value)| value);
    let value = values
        .next()
        .ok_or(TasmotaResponseError::MissingPulseTime)?;
    if values.next().is_some() {
        return Err(TasmotaResponseError::InvalidPulseTime);
    }
    let set = value
        .as_object()
        .and_then(|pulse_time| pulse_time.get("Set"))
        .and_then(Value::as_u64)
        .and_then(|set| u32::try_from(set).ok())
        .ok_or(TasmotaResponseError::InvalidPulseTime)?;

    Ok(PulseTimeReply { set })
}

pub fn parse_power_on_state_response(
    body: &str,
) -> Result<PowerOnStateReply, TasmotaResponseError> {
    let object = parse_object(body)?;
    let state = object
        .get("PowerOnState")
        .and_then(Value::as_u64)
        .and_then(|state| u8::try_from(state).ok())
        .filter(|state| *state <= 5)
        .ok_or_else(|| {
            if object.contains_key("PowerOnState") {
                TasmotaResponseError::InvalidPowerOnState
            } else {
                TasmotaResponseError::MissingPowerOnState
            }
        })?;

    Ok(PowerOnStateReply { state })
}

fn parse_object(body: &str) -> Result<Map<String, Value>, TasmotaResponseError> {
    match serde_json::from_str(body).map_err(|_| TasmotaResponseError::InvalidJson)? {
        Value::Object(object) => Ok(object),
        _ => Err(TasmotaResponseError::ExpectedObject),
    }
}

fn is_indexed_key(key: &str, stem: &str) -> bool {
    match key.strip_prefix(stem) {
        Some("") => true,
        Some(suffix) => suffix.chars().all(|character| character.is_ascii_digit()),
        None => false,
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
    const TRUNCATED: &str = r#"{"POWER":"ON"#;

    #[test]
    fn parses_power_on_and_off_replies() {
        assert_eq!(
            parse_power_response(POWER_ON).unwrap(),
            TasmotaReply {
                power: PowerState::On
            }
        );
        assert_eq!(
            parse_power_response(POWER_OFF).unwrap(),
            TasmotaReply {
                power: PowerState::Off
            }
        );
    }

    #[test]
    fn parses_an_indexed_power_reply() {
        assert_eq!(
            parse_power_response(POWER1_ON).unwrap(),
            TasmotaReply {
                power: PowerState::On
            }
        );
    }

    #[test]
    fn tolerates_extra_fields() {
        let body = r#"{"Time":"2026-09-06T10:00:00","Uptime":"0T00:01:02","POWER":"ON","Wifi":{"RSSI":72}}"#;

        assert_eq!(
            parse_power_response(body).unwrap(),
            TasmotaReply {
                power: PowerState::On
            }
        );
    }

    #[test]
    fn rejects_replies_without_a_power_state() {
        for body in [PULSETIME, POWER_ON_STATE, UNKNOWN_COMMAND] {
            assert_eq!(
                parse_power_response(body),
                Err(TasmotaResponseError::MissingPowerState)
            );
        }
    }

    #[test]
    fn rejects_malformed_or_non_object_json() {
        for body in [TRUNCATED, "", "not json", r#"{"POWER":"ON"} trailing"#] {
            assert_eq!(
                parse_power_response(body),
                Err(TasmotaResponseError::InvalidJson)
            );
        }
        assert_eq!(
            parse_power_response("[]"),
            Err(TasmotaResponseError::ExpectedObject)
        );
    }

    #[test]
    fn a_contradictory_power_state_never_confirms() {
        assert!(!parse_power_response(POWER_OFF).unwrap().confirms(true));
        assert!(!parse_power_response(POWER_ON).unwrap().confirms(false));
    }

    #[test]
    fn a_matching_power_state_confirms() {
        assert!(parse_power_response(POWER_ON).unwrap().confirms(true));
        assert!(parse_power_response(POWER_OFF).unwrap().confirms(false));
    }

    #[test]
    fn ignores_unrelated_keys_with_a_power_prefix() {
        assert_eq!(
            parse_power_response(r#"{"POWERUP":"ON","POWER":"OFF"}"#).unwrap(),
            TasmotaReply {
                power: PowerState::Off
            }
        );
    }

    #[test]
    fn rejects_invalid_and_conflicting_power_states() {
        assert_eq!(
            parse_power_response(r#"{"POWER":true}"#),
            Err(TasmotaResponseError::InvalidPowerState)
        );
        assert_eq!(
            parse_power_response(r#"{"POWER":"UNKNOWN"}"#),
            Err(TasmotaResponseError::InvalidPowerState)
        );
        assert_eq!(
            parse_power_response(r#"{"POWER":"ON","POWER1":"OFF"}"#),
            Err(TasmotaResponseError::ConflictingPowerStates)
        );
    }

    #[test]
    fn does_not_find_power_text_nested_in_an_unrelated_value() {
        assert_eq!(
            parse_power_response(r#"{"message":"reply was {\"POWER\":\"ON\"}"}"#),
            Err(TasmotaResponseError::MissingPowerState)
        );
    }

    #[test]
    fn parses_and_confirms_pulse_time_replies() {
        let reply = parse_pulse_time_response(PULSETIME).unwrap();

        assert_eq!(reply, PulseTimeReply { set: 120 });
        assert!(reply.confirms(120));
        assert!(!reply.confirms(121));
    }

    #[test]
    fn parses_an_indexed_pulse_time_reply() {
        assert_eq!(
            parse_pulse_time_response(r#"{"PulseTime1":{"Set":120,"Remaining":0}}"#),
            Ok(PulseTimeReply { set: 120 })
        );
    }

    #[test]
    fn rejects_missing_or_invalid_pulse_time_values() {
        assert_eq!(
            parse_pulse_time_response(UNKNOWN_COMMAND),
            Err(TasmotaResponseError::MissingPulseTime)
        );
        for body in [
            r#"{"PulseTime":{"Remaining":0}}"#,
            r#"{"PulseTime":{"Set":"120"}}"#,
            r#"{"PulseTime":120}"#,
            r#"{"PulseTime":{"Set":120},"PulseTime1":{"Set":120}}"#,
        ] {
            assert_eq!(
                parse_pulse_time_response(body),
                Err(TasmotaResponseError::InvalidPulseTime)
            );
        }
    }

    #[test]
    fn parses_and_confirms_power_on_state_replies() {
        let reply = parse_power_on_state_response(POWER_ON_STATE).unwrap();

        assert_eq!(reply, PowerOnStateReply { state: 0 });
        assert!(reply.confirms(0));
        assert!(!reply.confirms(1));
    }

    #[test]
    fn rejects_missing_or_invalid_power_on_state_values() {
        assert_eq!(
            parse_power_on_state_response(UNKNOWN_COMMAND),
            Err(TasmotaResponseError::MissingPowerOnState)
        );
        for body in [
            r#"{"PowerOnState":"0"}"#,
            r#"{"PowerOnState":-1}"#,
            r#"{"PowerOnState":6}"#,
        ] {
            assert_eq!(
                parse_power_on_state_response(body),
                Err(TasmotaResponseError::InvalidPowerOnState)
            );
        }
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

    #[test]
    fn builds_configuration_command_urls() {
        assert_eq!(
            pulse_time_command_url("http://192.168.8.193", 120),
            "http://192.168.8.193/cm?cmnd=PulseTime%20120"
        );
        assert_eq!(
            power_on_state_command_url("http://192.168.8.193", 0),
            "http://192.168.8.193/cm?cmnd=PowerOnState%200"
        );
    }
}
