#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvironmentState {
    pub time_s: f32,
    pub room_air_temp_c: f32,
    pub box_air_temp_c: f32,
    pub tempeh_core_temp_c: f32,
    pub fermentation_progress: f32,
    pub metabolic_heat_rate_c_per_s: f32,
    pub heater_on: bool,
}

impl EnvironmentState {
    pub fn csv_header() -> &'static str {
        "time_s,room_air_temp_c,box_air_temp_c,tempeh_core_temp_c,fermentation_progress,metabolic_heat_rate_c_per_s,heater_on"
    }

    pub fn csv_row(&self) -> String {
        format!(
            "{:.0},{:.3},{:.3},{:.3},{:.5},{:.8},{}",
            self.time_s,
            self.room_air_temp_c,
            self.box_air_temp_c,
            self.tempeh_core_temp_c,
            self.fermentation_progress,
            self.metabolic_heat_rate_c_per_s,
            if self.heater_on { 1 } else { 0 },
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControllerConfig {
    pub target_box_air_temp_c: f32,
    pub hysteresis_c: f32,
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            target_box_air_temp_c: 30.0,
            hysteresis_c: 0.4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TemperatureReading {
    pub time_s: f32,
    pub box_air_temp_c: f32,
    pub tempeh_core_temp_c: Option<f32>,
}

impl TemperatureReading {
    pub fn csv_header() -> &'static str {
        "time_s,box_air_temp_c,tempeh_core_temp_c"
    }

    pub fn csv_row(&self) -> String {
        let core = self
            .tempeh_core_temp_c
            .map(|temp| format!("{temp:.3}"))
            .unwrap_or_default();
        format!("{:.0},{:.3},{}", self.time_s, self.box_air_temp_c, core,)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TemperatureProbe {
    BoxAir,
    Product,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_state_writes_csv_header() {
        assert_eq!(
            EnvironmentState::csv_header(),
            "time_s,room_air_temp_c,box_air_temp_c,tempeh_core_temp_c,fermentation_progress,metabolic_heat_rate_c_per_s,heater_on"
        );
    }

    #[test]
    fn environment_state_writes_csv_row() {
        let state = EnvironmentState {
            time_s: 10.0,
            room_air_temp_c: 20.0,
            box_air_temp_c: 30.0,
            tempeh_core_temp_c: 29.0,
            fermentation_progress: 0.25,
            metabolic_heat_rate_c_per_s: 0.0001,
            heater_on: true,
        };
        assert!(state.csv_row().ends_with(",1"));
    }

    #[test]
    fn controller_config_default_is_expected() {
        assert_eq!(
            ControllerConfig::default(),
            ControllerConfig {
                target_box_air_temp_c: 30.0,
                hysteresis_c: 0.4,
            }
        );
    }

    #[test]
    fn temperature_reading_writes_csv_header() {
        assert_eq!(
            TemperatureReading::csv_header(),
            "time_s,box_air_temp_c,tempeh_core_temp_c"
        );
    }

    #[test]
    fn temperature_reading_writes_csv_row_with_core() {
        let reading = TemperatureReading {
            time_s: 2.0,
            box_air_temp_c: 22.4,
            tempeh_core_temp_c: Some(23.1),
        };
        assert_eq!(reading.csv_row(), "2,22.400,23.100");
    }

    #[test]
    fn temperature_reading_writes_csv_row_without_core() {
        let reading = TemperatureReading {
            time_s: 2.0,
            box_air_temp_c: 22.4,
            tempeh_core_temp_c: None,
        };
        assert_eq!(reading.csv_row(), "2,22.400,");
    }
}
