use serde_json::{Value, json};

use crate::mqtt::{MqttProtocolError, availability_topic, state_topic, validate_device_id};

const DISCOVERY_PREFIX: &str = "homeassistant";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryMessage {
    pub topic: String,
    pub payload: String,
}

pub fn discovery_messages(
    device_id: &str,
    device_name: &str,
    room_air_enabled: bool,
    product_enabled: bool,
) -> Result<Vec<DiscoveryMessage>, MqttProtocolError> {
    validate_device_id(device_id)?;

    let mut messages = Vec::with_capacity(9);
    messages.push(temperature_discovery(
        device_id,
        device_name,
        "box_air_temperature",
        "Box air temperature",
        "box_air_temp_c",
    )?);

    messages.push(if room_air_enabled {
        temperature_discovery(
            device_id,
            device_name,
            "room_air_temperature",
            "Room air temperature",
            "room_air_temp_c",
        )?
    } else {
        remove_discovery("sensor", device_id, "room_air_temperature")
    });

    messages.push(if product_enabled {
        temperature_discovery(
            device_id,
            device_name,
            "product_temperature",
            "Product temperature",
            "product_temp_c",
        )?
    } else {
        remove_discovery("sensor", device_id, "product_temperature")
    });

    messages.push(sensor_discovery(
        device_id,
        device_name,
        "run_state",
        "Run state",
        "run_state",
        None,
    )?);
    messages.push(binary_sensor_discovery(
        device_id,
        device_name,
        "heater_demand",
        "Heater demand",
        "{{ 'ON' if value_json.desired_heater_on else 'OFF' }}",
    )?);
    messages.push(sensor_discovery(
        device_id,
        device_name,
        "confirmed_heater",
        "Confirmed heater",
        "confirmed_heater",
        Some("diagnostic"),
    )?);
    messages.push(sensor_discovery(
        device_id,
        device_name,
        "fault_reason",
        "Fault reason",
        "fault_reason",
        Some("diagnostic"),
    )?);
    messages.push(binary_sensor_discovery(
        device_id,
        device_name,
        "actuator_ready",
        "Actuator ready",
        "{{ 'ON' if value_json.actuator_ready else 'OFF' }}",
    )?);
    messages.push(uptime_discovery(device_id, device_name)?);

    Ok(messages)
}

fn temperature_discovery(
    device_id: &str,
    device_name: &str,
    object_id: &str,
    name: &str,
    value_key: &str,
) -> Result<DiscoveryMessage, MqttProtocolError> {
    discovery_message(
        "sensor",
        device_id,
        object_id,
        json!({
            "name": name,
            "device_class": "temperature",
            "state_class": "measurement",
            "unit_of_measurement": "°C",
            "value_template": format!("{{{{ value_json.{value_key} }}}}"),
        }),
        device_name,
    )
}

fn sensor_discovery(
    device_id: &str,
    device_name: &str,
    object_id: &str,
    name: &str,
    value_key: &str,
    entity_category: Option<&str>,
) -> Result<DiscoveryMessage, MqttProtocolError> {
    let mut component = json!({
        "name": name,
        "value_template": format!("{{{{ value_json.{value_key} }}}}"),
    });
    if let Some(category) = entity_category {
        component["entity_category"] = Value::String(category.to_string());
    }

    discovery_message("sensor", device_id, object_id, component, device_name)
}

fn binary_sensor_discovery(
    device_id: &str,
    device_name: &str,
    object_id: &str,
    name: &str,
    value_template: &str,
) -> Result<DiscoveryMessage, MqttProtocolError> {
    discovery_message(
        "binary_sensor",
        device_id,
        object_id,
        json!({
            "name": name,
            "payload_on": "ON",
            "payload_off": "OFF",
            "value_template": value_template,
        }),
        device_name,
    )
}

fn uptime_discovery(
    device_id: &str,
    device_name: &str,
) -> Result<DiscoveryMessage, MqttProtocolError> {
    discovery_message(
        "sensor",
        device_id,
        "uptime",
        json!({
            "name": "Uptime",
            "device_class": "duration",
            "entity_category": "diagnostic",
            "unit_of_measurement": "s",
            "value_template": "{{ value_json.uptime_s }}",
        }),
        device_name,
    )
}

fn discovery_message(
    component: &str,
    device_id: &str,
    object_id: &str,
    mut payload: Value,
    device_name: &str,
) -> Result<DiscoveryMessage, MqttProtocolError> {
    let state_topic = state_topic(device_id)?;
    let availability_topic = availability_topic(device_id)?;
    let unique_id = format!("{device_id}_{object_id}");

    payload["unique_id"] = Value::String(unique_id);
    payload["state_topic"] = Value::String(state_topic);
    payload["availability_topic"] = Value::String(availability_topic);
    payload["payload_available"] = Value::String("online".to_string());
    payload["payload_not_available"] = Value::String("offline".to_string());
    payload["device"] = json!({
        "identifiers": [device_id],
        "name": device_name,
        "manufacturer": "Tempeh OS",
        "model": "Nologo ESP32-S3 SuperMini",
        "sw_version": env!("CARGO_PKG_VERSION"),
    });

    Ok(DiscoveryMessage {
        topic: format!("{DISCOVERY_PREFIX}/{component}/{device_id}/{object_id}/config"),
        payload: payload.to_string(),
    })
}

fn remove_discovery(component: &str, device_id: &str, object_id: &str) -> DiscoveryMessage {
    DiscoveryMessage {
        topic: format!("{DISCOVERY_PREFIX}/{component}/{device_id}/{object_id}/config"),
        payload: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_matches_enabled_probes_and_shared_topics() {
        let messages =
            discovery_messages("tempeh_controller", "Tempeh Controller", false, true).unwrap();

        assert_eq!(messages.len(), 9);
        assert!(
            messages
                .iter()
                .any(|message| message.topic.ends_with("/box_air_temperature/config"))
        );
        assert!(
            messages
                .iter()
                .any(|message| message.topic.ends_with("/product_temperature/config"))
        );
        let removed_room = messages
            .iter()
            .find(|message| message.topic.ends_with("/room_air_temperature/config"))
            .unwrap();
        assert!(removed_room.payload.is_empty());

        for message in messages
            .into_iter()
            .filter(|message| !message.payload.is_empty())
        {
            let payload: Value = serde_json::from_str(&message.payload).unwrap();
            assert_eq!(payload["state_topic"], "tempeh/tempeh_controller/state");
            assert_eq!(
                payload["availability_topic"],
                "tempeh/tempeh_controller/availability"
            );
            assert_eq!(payload["device"]["name"], "Tempeh Controller");
        }
    }
}
