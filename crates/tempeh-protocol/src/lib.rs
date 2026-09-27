pub mod home_assistant;
pub mod mqtt;
pub mod status;
pub mod tasmota;

use tempeh_model::TemperatureProbe;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProtocolError {
    InvalidTemperatureLine,
    InvalidControlLine,
    InvalidStateLine,
    InvalidActuatorLine,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParsedTemperatureLine {
    pub probe: TemperatureProbe,
    pub temp_c: f32,
}

pub fn parse_temperature_line(line: &str) -> Result<Option<ParsedTemperatureLine>, ProtocolError> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }

    let mut parts = line.split(',').map(str::trim);

    let Some(kind) = parts.next() else {
        return Ok(None);
    };

    if kind != "temp" {
        return Ok(None);
    }

    let Some(probe_name) = parts.next() else {
        return Err(ProtocolError::InvalidTemperatureLine);
    };

    let Some(temp_text) = parts.next() else {
        return Err(ProtocolError::InvalidTemperatureLine);
    };

    if parts.next().is_some() {
        return Err(ProtocolError::InvalidTemperatureLine);
    }

    let probe = match probe_name {
        "room_air" => TemperatureProbe::RoomAir,
        "box_air" => TemperatureProbe::BoxAir,
        "product" | "tempeh_core" => TemperatureProbe::Product,
        _ => return Ok(None),
    };

    let temp_c = temp_text
        .parse::<f32>()
        .map_err(|_| ProtocolError::InvalidTemperatureLine)?;

    if !temp_c.is_finite() {
        return Err(ProtocolError::InvalidTemperatureLine);
    }

    Ok(Some(ParsedTemperatureLine { probe, temp_c }))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedControlLine {
    pub time_s: f32,
    pub room_air_temp_c: Option<f32>,
    pub box_air_temp_c: f32,
    pub product_temp_c: Option<f32>,
    pub heater_on: bool,
    pub reason: String,
}

pub fn parse_control_line(line: &str) -> Result<Option<ParsedControlLine>, ProtocolError> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }

    let mut parts = line.split(',').map(str::trim);

    let Some(kind) = parts.next() else {
        return Ok(None);
    };

    if kind != "control" {
        return Ok(None);
    }

    let Some(time_text) = parts.next() else {
        return Err(ProtocolError::InvalidControlLine);
    };
    let Some(room_text) = parts.next() else {
        return Err(ProtocolError::InvalidControlLine);
    };
    let Some(box_air_text) = parts.next() else {
        return Err(ProtocolError::InvalidControlLine);
    };
    let Some(product_text) = parts.next() else {
        return Err(ProtocolError::InvalidControlLine);
    };
    let Some(heater_text) = parts.next() else {
        return Err(ProtocolError::InvalidControlLine);
    };

    let reason = parts.collect::<Vec<_>>().join(",");
    if reason.is_empty() {
        return Err(ProtocolError::InvalidControlLine);
    }

    let time_s = time_text
        .parse::<f32>()
        .map_err(|_| ProtocolError::InvalidControlLine)?;
    if !time_s.is_finite() {
        return Err(ProtocolError::InvalidControlLine);
    }

    let room_air_temp_c = parse_optional_temperature(room_text)?;
    let box_air_temp_c = box_air_text
        .parse::<f32>()
        .map_err(|_| ProtocolError::InvalidControlLine)?;
    if !box_air_temp_c.is_finite() {
        return Err(ProtocolError::InvalidControlLine);
    }
    let product_temp_c = parse_optional_temperature(product_text)?;

    let heater_on = match heater_text {
        "0" => false,
        "1" => true,
        _ => return Err(ProtocolError::InvalidControlLine),
    };

    Ok(Some(ParsedControlLine {
        time_s,
        room_air_temp_c,
        box_air_temp_c,
        product_temp_c,
        heater_on,
        reason,
    }))
}

fn parse_optional_temperature(text: &str) -> Result<Option<f32>, ProtocolError> {
    if text.is_empty() {
        return Ok(None);
    }

    let temp_c = text
        .parse::<f32>()
        .map_err(|_| ProtocolError::InvalidControlLine)?;
    if !temp_c.is_finite() {
        return Err(ProtocolError::InvalidControlLine);
    }

    Ok(Some(temp_c))
}

pub fn format_control_line(
    time_s: f32,
    room_air_temp_c: Option<f32>,
    box_air_temp_c: f32,
    product_temp_c: Option<f32>,
    heater_on: bool,
    reason: &str,
) -> String {
    format!(
        "control,{time_s:.0},{},{box_air_temp_c:.3},{},{},{reason}",
        format_optional_temperature(room_air_temp_c),
        format_optional_temperature(product_temp_c),
        if heater_on { 1 } else { 0 },
    )
}

fn format_optional_temperature(temp_c: Option<f32>) -> String {
    temp_c
        .map(|temp_c| format!("{temp_c:.3}"))
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedStateLine {
    pub time_s: f32,
    pub state: String,
    pub reason: String,
}

pub fn format_state_line(time_s: f32, state: &str, reason: &str) -> String {
    format!("state,{time_s:.0},{state},{reason}")
}

pub fn parse_state_line(line: &str) -> Result<Option<ParsedStateLine>, ProtocolError> {
    let Some(mut parts) = split_record(line, "state") else {
        return Ok(None);
    };

    let Some(time_text) = parts.next() else {
        return Err(ProtocolError::InvalidStateLine);
    };
    let Some(state) = parts.next() else {
        return Err(ProtocolError::InvalidStateLine);
    };

    let reason = parts.collect::<Vec<_>>().join(",");
    if state.is_empty() || reason.is_empty() {
        return Err(ProtocolError::InvalidStateLine);
    }

    let time_s = parse_time(time_text).ok_or(ProtocolError::InvalidStateLine)?;

    Ok(Some(ParsedStateLine {
        time_s,
        state: state.to_string(),
        reason,
    }))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedActuatorLine {
    pub time_s: f32,
    pub desired_heater_on: bool,
    pub confirmed_heater_on: Option<bool>,
    pub reason: String,
}

pub fn format_actuator_line(
    time_s: f32,
    desired_heater_on: bool,
    confirmed_heater_on: Option<bool>,
    reason: &str,
) -> String {
    format!(
        "actuator,{time_s:.0},{},{},{reason}",
        if desired_heater_on { 1 } else { 0 },
        format_optional_flag(confirmed_heater_on),
    )
}

pub fn parse_actuator_line(line: &str) -> Result<Option<ParsedActuatorLine>, ProtocolError> {
    let Some(mut parts) = split_record(line, "actuator") else {
        return Ok(None);
    };

    let Some(time_text) = parts.next() else {
        return Err(ProtocolError::InvalidActuatorLine);
    };
    let Some(desired_text) = parts.next() else {
        return Err(ProtocolError::InvalidActuatorLine);
    };
    let Some(confirmed_text) = parts.next() else {
        return Err(ProtocolError::InvalidActuatorLine);
    };

    let reason = parts.collect::<Vec<_>>().join(",");
    if reason.is_empty() {
        return Err(ProtocolError::InvalidActuatorLine);
    }

    let time_s = parse_time(time_text).ok_or(ProtocolError::InvalidActuatorLine)?;
    let desired_heater_on = parse_flag(desired_text).ok_or(ProtocolError::InvalidActuatorLine)?;
    let confirmed_heater_on =
        parse_optional_flag(confirmed_text).ok_or(ProtocolError::InvalidActuatorLine)?;

    Ok(Some(ParsedActuatorLine {
        time_s,
        desired_heater_on,
        confirmed_heater_on,
        reason,
    }))
}

fn split_record<'a>(line: &'a str, kind: &str) -> Option<impl Iterator<Item = &'a str> + use<'a>> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    let mut parts = line.split(',').map(str::trim);
    if parts.next()? != kind {
        return None;
    }

    Some(parts)
}

fn parse_time(text: &str) -> Option<f32> {
    text.parse::<f32>().ok().filter(|time_s| time_s.is_finite())
}

fn parse_flag(text: &str) -> Option<bool> {
    match text {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

fn format_optional_flag(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "1",
        Some(false) => "0",
        None => "",
    }
}

fn parse_optional_flag(text: &str) -> Option<Option<bool>> {
    if text.is_empty() {
        Some(None)
    } else {
        parse_flag(text).map(Some)
    }
}

pub fn format_temperature_line(probe: TemperatureProbe, temp_c: f32) -> String {
    format!("temp,{},{temp_c:.3}", probe_name(probe))
}

pub fn probe_name(probe: TemperatureProbe) -> &'static str {
    match probe {
        TemperatureProbe::RoomAir => "room_air",
        TemperatureProbe::BoxAir => "box_air",
        TemperatureProbe::Product => "product",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_box_air_temperature_line() {
        let parsed = parse_temperature_line("temp,box_air,22.437")
            .unwrap()
            .expect("temperature line");

        assert_eq!(
            parsed,
            ParsedTemperatureLine {
                probe: TemperatureProbe::BoxAir,
                temp_c: 22.437,
            }
        );
    }

    #[test]
    fn parses_room_air_temperature_line() {
        let parsed = parse_temperature_line("temp,room_air,20.125")
            .unwrap()
            .expect("temperature line");

        assert_eq!(
            parsed,
            ParsedTemperatureLine {
                probe: TemperatureProbe::RoomAir,
                temp_c: 20.125,
            }
        );
    }

    #[test]
    fn parses_product_temperature_line() {
        let parsed = parse_temperature_line("temp,product,23.125")
            .unwrap()
            .expect("temperature line");

        assert_eq!(parsed.probe, TemperatureProbe::Product);
        assert_eq!(parsed.temp_c, 23.125);
    }

    #[test]
    fn parses_legacy_tempeh_core_temperature_line_as_product() {
        let parsed = parse_temperature_line("temp,tempeh_core,23.125")
            .unwrap()
            .expect("temperature line");

        assert_eq!(parsed.probe, TemperatureProbe::Product);
        assert_eq!(parsed.temp_c, 23.125);
    }

    #[test]
    fn ignores_unknown_line_kind() {
        assert_eq!(parse_temperature_line("hello,world").unwrap(), None);
    }

    #[test]
    fn ignores_unknown_probe_name() {
        assert_eq!(parse_temperature_line("temp,outside,21.0").unwrap(), None);
    }

    #[test]
    fn rejects_bad_temperature_value() {
        assert_eq!(
            parse_temperature_line("temp,box_air,nope"),
            Err(ProtocolError::InvalidTemperatureLine)
        );
    }

    #[test]
    fn rejects_non_finite_temperature_value() {
        assert_eq!(
            parse_temperature_line("temp,box_air,NaN"),
            Err(ProtocolError::InvalidTemperatureLine)
        );
    }

    #[test]
    fn rejects_extra_fields() {
        assert_eq!(
            parse_temperature_line("temp,box_air,22.4,extra"),
            Err(ProtocolError::InvalidTemperatureLine)
        );
    }

    #[test]
    fn parses_control_line_with_optional_temperatures() {
        let parsed = parse_control_line("control,1,,22.437,23.125,1,below_target")
            .unwrap()
            .expect("control line");

        assert_eq!(
            parsed,
            ParsedControlLine {
                time_s: 1.0,
                room_air_temp_c: None,
                box_air_temp_c: 22.437,
                product_temp_c: Some(23.125),
                heater_on: true,
                reason: "below_target".to_string(),
            }
        );
    }

    #[test]
    fn ignores_unknown_line_kind_for_control_parser() {
        assert_eq!(parse_control_line("temp,box_air,22.4").unwrap(), None);
    }

    #[test]
    fn rejects_malformed_control_line() {
        assert_eq!(
            parse_control_line("control,1,,22.437,23.125,2,below_target"),
            Err(ProtocolError::InvalidControlLine)
        );
    }

    #[test]
    fn formats_temperature_lines() {
        assert_eq!(
            format_temperature_line(TemperatureProbe::BoxAir, 22.4374),
            "temp,box_air,22.437"
        );
        assert_eq!(
            format_temperature_line(TemperatureProbe::RoomAir, 20.125),
            "temp,room_air,20.125"
        );
        assert_eq!(
            format_temperature_line(TemperatureProbe::Product, 23.1),
            "temp,product,23.100"
        );
    }

    #[test]
    fn formats_control_lines_with_all_temperatures() {
        assert_eq!(
            format_control_line(2.0, Some(20.2), 22.4, Some(23.1), true, "below_target"),
            "control,2,20.200,22.400,23.100,1,below_target"
        );
    }

    #[test]
    fn formats_control_lines_without_optional_temperatures() {
        assert_eq!(
            format_control_line(2.0, None, 22.4, None, false, "holding_off"),
            "control,2,,22.400,,0,holding_off"
        );
        assert_eq!(
            format_control_line(2.0, Some(20.2), 22.4, None, false, "holding_off"),
            "control,2,20.200,22.400,,0,holding_off"
        );
        assert_eq!(
            format_control_line(2.0, None, 22.4, Some(23.1), false, "holding_off"),
            "control,2,,22.400,23.100,0,holding_off"
        );
    }

    #[test]
    fn formatted_control_lines_round_trip() {
        let line = format_control_line(12.0, None, 22.437, Some(23.125), true, "below_target");

        assert_eq!(
            parse_control_line(&line).unwrap(),
            Some(ParsedControlLine {
                time_s: 12.0,
                room_air_temp_c: None,
                box_air_temp_c: 22.437,
                product_temp_c: Some(23.125),
                heater_on: true,
                reason: "below_target".to_string(),
            })
        );
    }

    #[test]
    fn formats_and_parses_state_lines() {
        assert_eq!(
            format_state_line(12.0, "idle", "boot"),
            "state,12,idle,boot"
        );
        assert_eq!(
            parse_state_line("state,721,fault,box_air_stale")
                .unwrap()
                .expect("state line"),
            ParsedStateLine {
                time_s: 721.0,
                state: "fault".to_string(),
                reason: "box_air_stale".to_string(),
            }
        );
    }

    #[test]
    fn formats_and_parses_actuator_lines() {
        assert_eq!(
            format_actuator_line(53.0, true, Some(true), "lease_renewed"),
            "actuator,53,1,1,lease_renewed"
        );
        assert_eq!(
            parse_actuator_line("actuator,721,0,0,fault_off")
                .unwrap()
                .expect("actuator line"),
            ParsedActuatorLine {
                time_s: 721.0,
                desired_heater_on: false,
                confirmed_heater_on: Some(false),
                reason: "fault_off".to_string(),
            }
        );
    }

    #[test]
    fn parses_an_actuator_line_where_desired_and_confirmed_disagree() {
        let parsed = parse_actuator_line("actuator,90,1,0,renewal_failed")
            .unwrap()
            .expect("actuator line");

        assert!(parsed.desired_heater_on);
        assert_eq!(parsed.confirmed_heater_on, Some(false));
    }

    #[test]
    fn formats_and_parses_an_unknown_confirmed_actuator_state() {
        assert_eq!(
            format_actuator_line(90.0, false, None, "command_failed"),
            "actuator,90,0,,command_failed"
        );

        let parsed = parse_actuator_line("actuator,90,0,,command_failed")
            .unwrap()
            .expect("actuator line");

        assert!(!parsed.desired_heater_on);
        assert_eq!(parsed.confirmed_heater_on, None);
    }

    #[test]
    fn rejects_malformed_state_and_actuator_lines() {
        assert_eq!(
            parse_state_line("state,12,idle"),
            Err(ProtocolError::InvalidStateLine)
        );
        assert_eq!(
            parse_state_line("state,nope,idle,boot"),
            Err(ProtocolError::InvalidStateLine)
        );
        assert_eq!(
            parse_actuator_line("actuator,53,1,2,lease_renewed"),
            Err(ProtocolError::InvalidActuatorLine)
        );
        assert_eq!(
            parse_actuator_line("actuator,53,1,1"),
            Err(ProtocolError::InvalidActuatorLine)
        );
    }

    #[test]
    fn existing_parsers_ignore_the_new_record_kinds() {
        assert_eq!(parse_control_line("state,12,idle,boot").unwrap(), None);
        assert_eq!(
            parse_control_line("actuator,53,1,1,lease_renewed").unwrap(),
            None
        );
        assert_eq!(parse_temperature_line("state,12,idle,boot").unwrap(), None);
        assert_eq!(
            parse_temperature_line("actuator,53,1,1,lease_renewed").unwrap(),
            None
        );
    }

    #[test]
    fn new_parsers_ignore_existing_record_kinds() {
        assert_eq!(parse_state_line("temp,box_air,22.4").unwrap(), None);
        assert_eq!(
            parse_state_line("control,1,,22.437,23.125,1,below_target").unwrap(),
            None
        );
        assert_eq!(parse_actuator_line("temp,box_air,22.4").unwrap(), None);
        assert_eq!(
            parse_actuator_line("control,1,,22.437,23.125,1,below_target").unwrap(),
            None
        );
    }
}
