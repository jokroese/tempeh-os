use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MqttProtocolError {
    InvalidDeviceId,
    InvalidState,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatePayload<'a> {
    pub time_s: f32,
    pub room_air_temp_c: Option<f32>,
    pub box_air_temp_c: Option<f32>,
    pub product_temp_c: Option<f32>,
    pub run_state: &'a str,
    pub fault_reason: Option<&'a str>,
    pub desired_heater_on: bool,
    pub confirmed_heater_on: Option<bool>,
    pub actuator_ready: bool,
}

pub fn validate_device_id(device_id: &str) -> Result<(), MqttProtocolError> {
    let valid = !device_id.is_empty()
        && device_id.len() <= 64
        && device_id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte)
        });

    if valid {
        Ok(())
    } else {
        Err(MqttProtocolError::InvalidDeviceId)
    }
}

pub fn base_topic(device_id: &str) -> Result<String, MqttProtocolError> {
    validate_device_id(device_id)?;
    Ok(format!("tempeh/{device_id}"))
}

pub fn state_topic(device_id: &str) -> Result<String, MqttProtocolError> {
    Ok(format!("{}/state", base_topic(device_id)?))
}

pub fn availability_topic(device_id: &str) -> Result<String, MqttProtocolError> {
    Ok(format!("{}/availability", base_topic(device_id)?))
}

pub fn state_payload(state: StatePayload<'_>) -> Result<String, MqttProtocolError> {
    if !valid_optional_temperature(state.room_air_temp_c)
        || !valid_optional_temperature(state.box_air_temp_c)
        || !valid_optional_temperature(state.product_temp_c)
        || !state.time_s.is_finite()
        || state.time_s < 0.0
        || !matches!(state.run_state, "idle" | "running" | "fault")
    {
        return Err(MqttProtocolError::InvalidState);
    }

    Ok(json!({
        "uptime_s": state.time_s.round() as u64,
        "room_air_temp_c": state.room_air_temp_c,
        "box_air_temp_c": state.box_air_temp_c,
        "product_temp_c": state.product_temp_c,
        "run_state": state.run_state,
        "fault_reason": state.fault_reason.unwrap_or("none"),
        "desired_heater_on": state.desired_heater_on,
        "confirmed_heater": match state.confirmed_heater_on {
            Some(true) => "on",
            Some(false) => "off",
            None => "unknown",
        },
        "actuator_ready": state.actuator_ready,
    })
    .to_string())
}

fn valid_optional_temperature(value: Option<f32>) -> bool {
    value.is_none_or(f32::is_finite)
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn validates_safe_topic_identifiers() {
        for valid in ["tempeh_controller", "tempeh-01", "box2"] {
            assert_eq!(validate_device_id(valid), Ok(()));
        }
        for invalid in ["", "Tempeh", "tempeh/controller", "tempeh+", "tempeh #"] {
            assert_eq!(
                validate_device_id(invalid),
                Err(MqttProtocolError::InvalidDeviceId)
            );
        }
    }

    #[test]
    fn formats_a_complete_state_payload() {
        let payload = state_payload(StatePayload {
            time_s: 12.6,
            room_air_temp_c: Some(20.125),
            box_air_temp_c: Some(24.625),
            product_temp_c: None,
            run_state: "running",
            fault_reason: None,
            desired_heater_on: true,
            confirmed_heater_on: Some(true),
            actuator_ready: true,
        })
        .unwrap();
        let json: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(json["uptime_s"], 13);
        assert_eq!(json["box_air_temp_c"], 24.625);
        assert!(json["product_temp_c"].is_null());
        assert_eq!(json["run_state"], "running");
        assert_eq!(json["fault_reason"], "none");
        assert_eq!(json["confirmed_heater"], "on");
    }

    #[test]
    fn formats_unknown_confirmation_and_fault() {
        let payload = state_payload(StatePayload {
            time_s: 5.0,
            room_air_temp_c: None,
            box_air_temp_c: Some(24.0),
            product_temp_c: None,
            run_state: "fault",
            fault_reason: Some("actuator_failed"),
            desired_heater_on: false,
            confirmed_heater_on: None,
            actuator_ready: false,
        })
        .unwrap();
        let json: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(json["confirmed_heater"], "unknown");
        assert_eq!(json["fault_reason"], "actuator_failed");
    }

    #[test]
    fn rejects_invalid_numeric_state() {
        let invalid = StatePayload {
            time_s: f32::NAN,
            room_air_temp_c: None,
            box_air_temp_c: Some(24.0),
            product_temp_c: None,
            run_state: "idle",
            fault_reason: None,
            desired_heater_on: false,
            confirmed_heater_on: Some(false),
            actuator_ready: true,
        };

        assert_eq!(state_payload(invalid), Err(MqttProtocolError::InvalidState));
    }
}
