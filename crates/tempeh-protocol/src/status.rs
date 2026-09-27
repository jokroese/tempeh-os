#[cfg(feature = "status-parse")]
use serde::Deserialize;
use serde::Serialize;

pub const STATUS_VERSION: u8 = 1;

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
    if record.version != STATUS_VERSION {
        return Err(format!("unsupported status version {}", record.version));
    }
    if record.boot_id.is_empty()
        || !record.uptime_s.is_finite()
        || record.uptime_s < 0.0
        || !record.lease_duration_s.is_finite()
        || record.lease_duration_s <= 0.0
        || !matches!(record.run_state.as_str(), "idle" | "running" | "fault")
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
    fn round_trips_and_rejects_unknown_version() {
        let record = StatusRecord {
            version: STATUS_VERSION,
            boot_id: "abc".into(),
            uptime_s: 25.0,
            run_state: "fault".into(),
            fault_reason: Some("actuator_failed".into()),
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
            Ok(Some(record))
        );
        assert!(parse_status_line("status,{\"version\":2}").is_err());
    }
}
