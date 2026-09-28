#[cfg(feature = "status-parse")]
use serde::Deserialize;
use serde::Serialize;

pub const STATUS_VERSION: u8 = 2;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "status-parse", derive(Deserialize))]
pub struct ProbeStatus {
    pub temp_c: Option<f32>,
    pub age_s: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "status-parse", derive(Deserialize))]
pub struct StatusRecord {
    pub version: u8,
    pub boot_id: String,
    pub uptime_s: f32,
    pub run_state: String,
    pub fault_reason: Option<String>,
    #[cfg_attr(feature = "status-parse", serde(default))]
    pub pause_reason: Option<String>,
    #[cfg_attr(feature = "status-parse", serde(default))]
    pub actuator_warning: Option<String>,
    #[cfg_attr(feature = "status-parse", serde(default))]
    pub interruption_started_s: Option<f32>,
    pub desired_heater_on: bool,
    pub actuator_ready: bool,
    pub confirmed_heater_on: Option<bool>,
    pub last_confirmed_heater_on: Option<bool>,
    pub last_successful_command_s: Option<f32>,
    pub last_successful_renewal_s: Option<f32>,
    pub lease_duration_s: f32,
    pub room_air: ProbeStatus,
    pub box_air: ProbeStatus,
    pub product: ProbeStatus,
}

pub fn format_status_line(record: &StatusRecord) -> String {
    format!(
        "status,{}",
        serde_json::to_string(record).expect("finite firmware status")
    )
}

#[cfg(feature = "status-parse")]
pub fn parse_status_line(line: &str) -> Result<Option<StatusRecord>, String> {
    let Some(body) = line.trim().strip_prefix("status,") else {
        return Ok(None);
    };
    let record: StatusRecord = serde_json::from_str(body).map_err(|error| error.to_string())?;
    if record.version != 1 && record.version != STATUS_VERSION {
        return Err(format!("unsupported status version {}", record.version));
    }
    if record.boot_id.is_empty()
        || !record.uptime_s.is_finite()
        || record.uptime_s < 0.0
        || !record.lease_duration_s.is_finite()
        || record.lease_duration_s <= 0.0
        || !matches!(
            record.run_state.as_str(),
            "idle" | "running" | "paused" | "fault"
        )
        || (record.version == 1 && record.run_state == "paused")
        || record
            .interruption_started_s
            .is_some_and(|value| !value.is_finite() || value < 0.0 || value > record.uptime_s)
        || record
            .last_successful_command_s
            .is_some_and(|value| !value.is_finite() || value < 0.0 || value > record.uptime_s)
        || record
            .last_successful_renewal_s
            .is_some_and(|value| !value.is_finite() || value < 0.0 || value > record.uptime_s)
        || [&record.room_air, &record.box_air, &record.product]
            .into_iter()
            .any(|probe| {
                probe.temp_c.is_some_and(|value| !value.is_finite())
                    || probe
                        .age_s
                        .is_some_and(|value| !value.is_finite() || value < 0.0)
            })
    {
        return Err("invalid status values".into());
    }
    Ok(Some(record))
}

#[cfg(all(test, feature = "status-parse"))]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_accepts_legacy_status() {
        let record = StatusRecord {
            version: STATUS_VERSION,
            boot_id: "abc".into(),
            uptime_s: 25.0,
            run_state: "fault".into(),
            fault_reason: Some("actuator_failed".into()),
            pause_reason: None,
            actuator_warning: None,
            interruption_started_s: None,
            desired_heater_on: false,
            actuator_ready: false,
            confirmed_heater_on: None,
            last_confirmed_heater_on: Some(true),
            last_successful_command_s: Some(2.0),
            last_successful_renewal_s: Some(2.0),
            lease_duration_s: 20.0,
            room_air: ProbeStatus {
                temp_c: None,
                age_s: None,
            },
            box_air: ProbeStatus {
                temp_c: Some(20.0),
                age_s: Some(1.0),
            },
            product: ProbeStatus {
                temp_c: Some(21.0),
                age_s: Some(1.0),
            },
        };
        assert_eq!(
            parse_status_line(&format_status_line(&record)),
            Ok(Some(record.clone()))
        );
        let mut legacy: serde_json::Value =
            serde_json::from_str(&format_status_line(&record)[7..]).unwrap();
        legacy["version"] = 1.into();
        for field in ["pause_reason", "actuator_warning", "interruption_started_s"] {
            legacy.as_object_mut().unwrap().remove(field);
        }
        let parsed = parse_status_line(&format!("status,{legacy}"))
            .unwrap()
            .unwrap();
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.pause_reason, None);
        assert!(parse_status_line("status,{\"version\":3}").is_err());
    }

    #[test]
    fn paused_status_round_trips() {
        let legacy = r#"{"version":1,"boot_id":"old","uptime_s":2,"run_state":"idle","fault_reason":null,"desired_heater_on":false,"actuator_ready":true,"confirmed_heater_on":false,"last_confirmed_heater_on":false,"last_successful_command_s":0,"last_successful_renewal_s":null,"lease_duration_s":20,"room_air":{"temp_c":null,"age_s":null},"box_air":{"temp_c":20,"age_s":0},"product":{"temp_c":null,"age_s":null}}"#;
        let mut paused: serde_json::Value = serde_json::from_str(legacy).unwrap();
        paused["version"] = STATUS_VERSION.into();
        paused["run_state"] = "paused".into();
        paused["pause_reason"] = "actuator_unreachable".into();
        paused["interruption_started_s"] = 1.0.into();
        let parsed = parse_status_line(&format!("status,{paused}"))
            .unwrap()
            .unwrap();
        assert_eq!(parsed.run_state, "paused");
        assert_eq!(parsed.pause_reason.as_deref(), Some("actuator_unreachable"));
    }
}
