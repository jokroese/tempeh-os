use tempeh_model::TemperatureProbe;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProtocolError {
    InvalidTemperatureLine,
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
}
