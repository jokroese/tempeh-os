use tempeh_model::ControllerConfig;
use tempeh_sim::TemperatureTrace;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThermometerError {
    Unavailable,
    InvalidReading,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeaterError {
    CommandFailed,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ControlError {
    Thermometer(ThermometerError),
    Heater(HeaterError),
}

#[derive(Debug, Clone, Copy)]
pub struct Controller {
    pub config: ControllerConfig,
    heater_on: bool,
}

impl Controller {
    pub fn new(config: ControllerConfig) -> Self {
        Self {
            config,
            heater_on: false,
        }
    }

    pub fn update(&mut self, box_air_temp_c: f32, tempeh_core_temp_c: f32) -> bool {
        if box_air_temp_c >= self.config.hard_box_cutoff_c
            || tempeh_core_temp_c >= self.config.hard_tempeh_cutoff_c
        {
            self.heater_on = false;
            return self.heater_on;
        }
        if !self.heater_on
            && box_air_temp_c < self.config.target_box_air_temp_c - self.config.hysteresis_c
        {
            self.heater_on = true;
        }
        if self.heater_on
            && box_air_temp_c > self.config.target_box_air_temp_c + self.config.hysteresis_c
        {
            self.heater_on = false;
        }
        self.heater_on
    }
}

impl From<ThermometerError> for ControlError {
    fn from(error: ThermometerError) -> Self {
        Self::Thermometer(error)
    }
}

impl From<HeaterError> for ControlError {
    fn from(error: HeaterError) -> Self {
        Self::Heater(error)
    }
}

pub trait Thermometer {
    fn read_celsius(&mut self) -> Result<f32, ThermometerError>;
}

pub trait Heater {
    fn set_heater(&mut self, on: bool) -> Result<(), HeaterError>;
    fn heater_on(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlReading {
    pub time_s: f32,
    pub measured_temp_c: f32,
    pub heater_on: bool,
}

impl ControlReading {
    pub fn csv_header() -> &'static str {
        "time_s,measured_temp_c,heater_on"
    }

    pub fn csv_row(&self) -> String {
        format!(
            "{:.0},{:.3},{}",
            self.time_s,
            self.measured_temp_c,
            if self.heater_on { 1 } else { 0 }
        )
    }
}

#[derive(Debug, Clone)]
pub struct ControlLoop<T, H>
where
    T: Thermometer,
    H: Heater,
{
    controller: Controller,
    thermometer: T,
    heater: H,
}

impl<T, H> ControlLoop<T, H>
where
    T: Thermometer,
    H: Heater,
{
    pub fn new(config: ControllerConfig, thermometer: T, heater: H) -> Self {
        Self {
            controller: Controller::new(config),
            thermometer,
            heater,
        }
    }

    /// Read one thermometer and update one heater.
    ///
    /// For this first hardware abstraction pass, the same measured temperature
    /// is used for both box air and tempeh-core inputs to the controller.
    /// Once we have multiple sensors, this should become a multi-probe control step.
    pub fn step(&mut self, time_s: f32) -> Result<ControlReading, ControlError> {
        let measured_temp_c = self.thermometer.read_celsius()?;

        if !measured_temp_c.is_finite() {
            self.heater.set_heater(false)?;
            return Err(ThermometerError::InvalidReading.into());
        }

        let heater_on = self.controller.update(measured_temp_c, measured_temp_c);
        self.heater.set_heater(heater_on)?;

        Ok(ControlReading {
            time_s,
            measured_temp_c,
            heater_on,
        })
    }

    pub fn heater(&self) -> &H {
        &self.heater
    }

    pub fn heater_mut(&mut self) -> &mut H {
        &mut self.heater
    }

    pub fn thermometer(&self) -> &T {
        &self.thermometer
    }

    pub fn thermometer_mut(&mut self) -> &mut T {
        &mut self.thermometer
    }
}

#[derive(Debug, Clone)]
pub struct TraceThermometer {
    trace: TemperatureTrace,
    index: usize,
}

impl TraceThermometer {
    pub fn new(trace: TemperatureTrace) -> Self {
        Self { trace, index: 0 }
    }

    pub fn len(&self) -> usize {
        self.trace.len()
    }

    pub fn is_empty(&self) -> bool {
        self.trace.is_empty()
    }
}

impl Thermometer for TraceThermometer {
    fn read_celsius(&mut self) -> Result<f32, ThermometerError> {
        let reading = self
            .trace
            .sample_or_last(self.index)
            .ok_or(ThermometerError::Unavailable)?;
        self.index = self.index.saturating_add(1);

        if reading.is_finite() {
            Ok(reading)
        } else {
            Err(ThermometerError::InvalidReading)
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LoggingHeater {
    heater_on: bool,
    commands: Vec<bool>,
}

impl LoggingHeater {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn commands(&self) -> &[bool] {
        &self.commands
    }
}

impl Heater for LoggingHeater {
    fn set_heater(&mut self, on: bool) -> Result<(), HeaterError> {
        self.heater_on = on;
        self.commands.push(on);
        Ok(())
    }

    fn heater_on(&self) -> bool {
        self.heater_on
    }
}

#[derive(Debug, Clone)]
pub struct TasmotaHeater {
    base_url: String,
    heater_on: bool,
}

impl TasmotaHeater {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: normalise_base_url(base_url.into()),
            heater_on: false,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn command_url(&self, on: bool) -> String {
        let command = if on { "Power%20On" } else { "Power%20Off" };
        format!("{}/cm?cmnd={command}", self.base_url)
    }
}

impl Heater for TasmotaHeater {
    fn set_heater(&mut self, on: bool) -> Result<(), HeaterError> {
        let url = self.command_url(on);

        ureq::get(&url)
            .call()
            .map_err(|_| HeaterError::CommandFailed)?;

        self.heater_on = on;
        Ok(())
    }

    fn heater_on(&self) -> bool {
        self.heater_on
    }
}

fn normalise_base_url(mut base_url: String) -> String {
    base_url = base_url.trim().trim_end_matches('/').to_string();

    if base_url.starts_with("http://") || base_url.starts_with("https://") {
        base_url
    } else {
        format!("http://{base_url}")
    }
}

#[derive(Debug, Clone)]
pub struct ControlRun {
    pub readings: Vec<ControlReading>,
    pub heater_commands: Vec<bool>,
}

pub fn run_trace_control(
    controller_config: ControllerConfig,
    trace: TemperatureTrace,
    dt_s: f32,
    duration_s: f32,
) -> Result<ControlRun, ControlError> {
    let thermometer = TraceThermometer::new(trace);
    let heater = LoggingHeater::new();
    let mut control = ControlLoop::new(controller_config, thermometer, heater);

    let mut readings = Vec::new();
    let mut time_s = 0.0;

    while time_s <= duration_s {
        readings.push(control.step(time_s)?);
        time_s += dt_s;
    }

    let heater_commands = control.heater().commands().to_vec();

    Ok(ControlRun {
        readings,
        heater_commands,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempeh_model::EnvironmentState;
    use tempeh_sim::SimConfig;

    #[test]
    fn trace_thermometer_returns_samples_in_order() {
        let trace = TemperatureTrace::new(vec![20.0, 21.5, 22.0]);
        let mut thermometer = TraceThermometer::new(trace);

        assert_eq!(thermometer.read_celsius().unwrap(), 20.0);
        assert_eq!(thermometer.read_celsius().unwrap(), 21.5);
        assert_eq!(thermometer.read_celsius().unwrap(), 22.0);
    }

    #[test]
    fn trace_thermometer_holds_last_sample() {
        let trace = TemperatureTrace::new(vec![20.0, 21.0]);
        let mut thermometer = TraceThermometer::new(trace);

        assert_eq!(thermometer.read_celsius().unwrap(), 20.0);
        assert_eq!(thermometer.read_celsius().unwrap(), 21.0);
        assert_eq!(thermometer.read_celsius().unwrap(), 21.0);
        assert_eq!(thermometer.read_celsius().unwrap(), 21.0);
    }

    #[test]
    fn logging_heater_records_commands() {
        let mut heater = LoggingHeater::new();

        heater.set_heater(true).unwrap();
        heater.set_heater(false).unwrap();

        assert!(!heater.heater_on());
        assert_eq!(heater.commands(), &[true, false]);
    }

    #[test]
    fn tasmota_heater_normalises_base_url() {
        let heater = TasmotaHeater::new("192.168.1.50/");
        assert_eq!(heater.base_url(), "http://192.168.1.50");
    }

    #[test]
    fn tasmota_heater_preserves_explicit_scheme() {
        let heater = TasmotaHeater::new("https://plug.local/");
        assert_eq!(heater.base_url(), "https://plug.local");
    }

    #[test]
    fn control_loop_turns_heater_on_when_cold() {
        let config = ControllerConfig::default();
        let thermometer = TraceThermometer::new(TemperatureTrace::new(vec![20.0]));
        let heater = LoggingHeater::new();
        let mut control = ControlLoop::new(config, thermometer, heater);

        let reading = control.step(0.0).unwrap();

        assert!(reading.heater_on);
        assert!(control.heater().heater_on());
    }

    #[test]
    fn control_loop_turns_heater_off_at_hard_cutoff() {
        let config = ControllerConfig::default();
        let thermometer = TraceThermometer::new(TemperatureTrace::new(vec![
            20.0,
            config.hard_box_cutoff_c + 0.5,
        ]));
        let heater = LoggingHeater::new();
        let mut control = ControlLoop::new(config, thermometer, heater);

        let first = control.step(0.0).unwrap();
        let second = control.step(10.0).unwrap();

        assert!(first.heater_on);
        assert!(!second.heater_on);
        assert!(!control.heater().heater_on());
    }

    #[test]
    fn controller_turns_off_at_hard_cutoff() {
        let config = ControllerConfig::default();
        let mut controller = Controller::new(config);
        assert!(controller.update(20.0, 20.0));
        assert!(!controller.update(config.hard_box_cutoff_c + 0.1, 20.0));
        assert!(!controller.update(20.0, config.hard_tempeh_cutoff_c + 0.1));
    }

    #[test]
    fn trace_control_run_produces_readings_and_commands() {
        let config = SimConfig {
            duration_s: 60.0,
            dt_s: 10.0,
            ..SimConfig::default()
        };
        let states = vec![
            EnvironmentState {
                time_s: 0.0,
                room_air_temp_c: 20.0,
                box_air_temp_c: 20.0,
                tempeh_core_temp_c: 20.0,
                fermentation_progress: 0.0,
                metabolic_heat_rate_c_per_s: 0.0,
                heater_on: false,
            },
            EnvironmentState {
                time_s: 10.0,
                room_air_temp_c: 20.0,
                box_air_temp_c: 21.0,
                tempeh_core_temp_c: 20.5,
                fermentation_progress: 0.0,
                metabolic_heat_rate_c_per_s: 0.0,
                heater_on: true,
            },
        ];
        let trace = TemperatureTrace::from_box_air_states(states);
        let run =
            run_trace_control(config.controller, trace, config.dt_s, config.duration_s).unwrap();

        assert!(!run.readings.is_empty());
        assert_eq!(run.readings.len(), run.heater_commands.len());
    }
}
