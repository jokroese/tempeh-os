use std::collections::VecDeque;

use serde_json::json;

pub const MAX_PENDING_EVENTS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MqttProtocolError {
    InvalidDeviceId,
    InvalidState,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatePayload<'a> {
    pub time_s: f32,
    pub boot_id: &'a str,
    pub room_air_temp_c: Option<f32>,
    pub room_air_age_s: Option<f32>,
    pub box_air_temp_c: Option<f32>,
    pub box_air_age_s: Option<f32>,
    pub product_temp_c: Option<f32>,
    pub product_age_s: Option<f32>,
    pub run_state: &'a str,
    pub fault_reason: Option<&'a str>,
    pub desired_heater_on: bool,
    pub confirmed_heater_on: Option<bool>,
    pub last_confirmed_heater_on: Option<bool>,
    pub confirmation_age_s: Option<f32>,
    pub lease_duration_s: f32,
    pub actuator_ready: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MqttEvent {
    pub sequence: u64,
    pub uptime_s: f32,
    pub event_type: &'static str,
    pub fault_reason: Option<&'static str>,
}

#[derive(Debug)]
pub struct EventBuffer {
    boot_id: String,
    current_fault: Option<&'static str>,
    next_sequence: u64,
    pending: VecDeque<MqttEvent>,
}

impl EventBuffer {
    pub fn new(boot_id: String, uptime_s: f32, initial_fault: Option<&'static str>) -> Self {
        let mut buffer = Self {
            boot_id,
            current_fault: None,
            next_sequence: 1,
            pending: VecDeque::with_capacity(MAX_PENDING_EVENTS),
        };
        buffer.push(uptime_s, "boot", None);
        buffer.observe_fault(uptime_s, initial_fault);
        buffer
    }

    pub fn boot_id(&self) -> &str {
        &self.boot_id
    }

    /// Returns the number of oldest events discarded to keep the buffer bounded.
    pub fn observe_fault(&mut self, uptime_s: f32, fault: Option<&'static str>) -> usize {
        if self.current_fault == fault {
            return 0;
        }

        let mut dropped = 0;
        if let Some(previous) = self.current_fault.take() {
            dropped += self.push(uptime_s, "fault_cleared", Some(previous));
        }
        if let Some(reason) = fault {
            dropped += self.push(uptime_s, "fault_raised", Some(reason));
            self.current_fault = Some(reason);
        }
        dropped
    }

    pub fn front(&self) -> Option<&MqttEvent> {
        self.pending.front()
    }

    pub fn pop_front(&mut self) {
        self.pending.pop_front();
    }

    fn push(
        &mut self,
        uptime_s: f32,
        event_type: &'static str,
        fault_reason: Option<&'static str>,
    ) -> usize {
        let dropped = usize::from(self.pending.len() == MAX_PENDING_EVENTS);
        if dropped != 0 {
            self.pending.pop_front();
        }
        self.pending.push_back(MqttEvent {
            sequence: self.next_sequence,
            uptime_s,
            event_type,
            fault_reason,
        });
        self.next_sequence += 1;
        dropped
    }
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

pub fn event_topic(device_id: &str) -> Result<String, MqttProtocolError> {
    Ok(format!("{}/event", base_topic(device_id)?))
}

pub fn event_payload(boot_id: &str, event: &MqttEvent) -> Result<String, MqttProtocolError> {
    if boot_id.is_empty() || !event.uptime_s.is_finite() || event.uptime_s < 0.0 {
        return Err(MqttProtocolError::InvalidState);
    }
    Ok(json!({
        "event_id": format!("{boot_id}:{}", event.sequence),
        "event_type": event.event_type,
        "boot_id": boot_id,
        "sequence": event.sequence,
        "uptime_s": event.uptime_s,
        "fault_reason": event.fault_reason,
    })
    .to_string())
}

pub fn state_payload(state: StatePayload<'_>) -> Result<String, MqttProtocolError> {
    if !valid_optional_temperature(state.room_air_temp_c)
        || !valid_optional_temperature(state.box_air_temp_c)
        || !valid_optional_temperature(state.product_temp_c)
        || !valid_optional_age(state.room_air_age_s)
        || !valid_optional_age(state.box_air_age_s)
        || !valid_optional_age(state.product_age_s)
        || !valid_optional_age(state.confirmation_age_s)
        || !state.lease_duration_s.is_finite()
        || state.lease_duration_s <= 0.0
        || state.boot_id.is_empty()
        || !state.time_s.is_finite()
        || state.time_s < 0.0
        || !matches!(state.run_state, "idle" | "running" | "fault")
    {
        return Err(MqttProtocolError::InvalidState);
    }

    Ok(json!({
        "uptime_s": state.time_s.round() as u64,
        "boot_id": state.boot_id,
        "room_air_temp_c": state.room_air_temp_c,
        "room_air_age_s": state.room_air_age_s,
        "box_air_temp_c": state.box_air_temp_c,
        "box_air_age_s": state.box_air_age_s,
        "product_temp_c": state.product_temp_c,
        "product_age_s": state.product_age_s,
        "run_state": state.run_state,
        "fault_reason": state.fault_reason.unwrap_or("none"),
        "desired_heater_on": state.desired_heater_on,
        "confirmed_heater": match state.confirmed_heater_on {
            Some(true) => "on",
            Some(false) => "off",
            None => "unknown",
        },
        "last_confirmed_heater": match state.last_confirmed_heater_on {
            Some(true) => "on",
            Some(false) => "off",
            None => "unknown",
        },
        "confirmation_age_s": state.confirmation_age_s,
        "lease_duration_s": state.lease_duration_s,
        "actuator_ready": state.actuator_ready,
    })
    .to_string())
}

fn valid_optional_temperature(value: Option<f32>) -> bool {
    value.is_none_or(f32::is_finite)
}

fn valid_optional_age(value: Option<f32>) -> bool {
    value.is_none_or(|age| age.is_finite() && age >= 0.0)
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
        assert_eq!(event_topic("box2").unwrap(), "tempeh/box2/event");
    }

    fn sample_state() -> StatePayload<'static> {
        StatePayload {
            time_s: 12.6,
            boot_id: "boot-abc",
            room_air_temp_c: Some(20.125),
            room_air_age_s: Some(3.5),
            box_air_temp_c: Some(24.625),
            box_air_age_s: Some(1.25),
            product_temp_c: None,
            product_age_s: None,
            run_state: "running",
            fault_reason: None,
            desired_heater_on: true,
            confirmed_heater_on: Some(true),
            last_confirmed_heater_on: Some(true),
            confirmation_age_s: Some(2.5),
            lease_duration_s: 20.0,
            actuator_ready: true,
        }
    }

    #[test]
    fn formats_a_complete_state_payload() {
        let payload = state_payload(sample_state()).unwrap();
        let json: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(json["uptime_s"], 13);
        assert_eq!(json["boot_id"], "boot-abc");
        assert_eq!(json["box_air_temp_c"], 24.625);
        assert_eq!(json["box_air_age_s"], 1.25);
        assert!(json["product_temp_c"].is_null());
        assert!(json["product_age_s"].is_null());
        assert_eq!(json["run_state"], "running");
        assert_eq!(json["fault_reason"], "none");
        assert_eq!(json["confirmed_heater"], "on");
        assert_eq!(json["last_confirmed_heater"], "on");
        assert_eq!(json["confirmation_age_s"], 2.5);
        assert_eq!(json["lease_duration_s"], 20.0);
    }

    #[test]
    fn formats_unknown_confirmation_and_fault() {
        let payload = state_payload(StatePayload {
            time_s: 25.0,
            room_air_temp_c: None,
            room_air_age_s: None,
            box_air_age_s: Some(21.0),
            run_state: "fault",
            fault_reason: Some("actuator_failed"),
            desired_heater_on: false,
            confirmed_heater_on: None,
            last_confirmed_heater_on: Some(true),
            confirmation_age_s: Some(22.0),
            actuator_ready: false,
            ..sample_state()
        })
        .unwrap();
        let json: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(json["confirmed_heater"], "unknown");
        assert_eq!(json["last_confirmed_heater"], "on");
        assert_eq!(json["confirmation_age_s"], 22.0);
        assert_eq!(json["box_air_age_s"], 21.0);
        assert_eq!(json["fault_reason"], "actuator_failed");
    }

    #[test]
    fn rejects_invalid_numeric_state() {
        let invalid = StatePayload {
            time_s: f32::NAN,
            ..sample_state()
        };

        assert_eq!(state_payload(invalid), Err(MqttProtocolError::InvalidState));
        assert_eq!(
            state_payload(StatePayload {
                box_air_age_s: Some(-1.0),
                ..sample_state()
            }),
            Err(MqttProtocolError::InvalidState)
        );
        assert_eq!(
            state_payload(StatePayload {
                boot_id: "",
                ..sample_state()
            }),
            Err(MqttProtocolError::InvalidState)
        );
    }

    #[test]
    fn buffers_boot_fault_and_clearance_in_order_through_reconnect() {
        let mut events = EventBuffer::new("boot-abc".into(), 1.0, Some("boot_config_failed"));
        assert_eq!(events.front().unwrap().event_type, "boot");
        let boot: Value = serde_json::from_str(
            &event_payload(events.boot_id(), events.front().unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(boot["event_id"], "boot-abc:1");
        assert_eq!(boot["sequence"], 1);
        events.pop_front();

        assert_eq!(events.front().unwrap().event_type, "fault_raised");
        assert_eq!(
            events.front().unwrap().fault_reason.as_deref(),
            Some("boot_config_failed")
        );
        events.observe_fault(5.0, Some("boot_config_failed"));
        events.observe_fault(8.0, None);
        // A disconnected publisher leaves the queue untouched until enqueue succeeds.
        assert_eq!(events.front().unwrap().sequence, 2);
        events.pop_front();
        assert_eq!(events.front().unwrap().event_type, "fault_cleared");
        assert_eq!(events.front().unwrap().sequence, 3);
        assert_eq!(
            events.front().unwrap().fault_reason.as_deref(),
            Some("boot_config_failed")
        );
        events.pop_front();
        assert!(events.front().is_none());
    }

    #[test]
    fn drops_oldest_events_when_buffer_is_full() {
        let mut events = EventBuffer::new("boot-x".into(), 0.0, None);
        let mut dropped = 0;
        for i in 0..MAX_PENDING_EVENTS {
            dropped += events.observe_fault((i * 2 + 1) as f32, Some("box_air_stale"));
            dropped += events.observe_fault((i * 2 + 2) as f32, None);
        }
        assert_eq!(dropped, MAX_PENDING_EVENTS + 1);
        assert_eq!(
            events.front().unwrap().sequence,
            (MAX_PENDING_EVENTS + 2) as u64
        );
        let mut count = 0;
        while events.front().is_some() {
            events.pop_front();
            count += 1;
        }
        assert_eq!(count, MAX_PENDING_EVENTS);
    }
}
