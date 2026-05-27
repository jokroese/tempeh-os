use tempehcore::{Controller, SimConfig, Simulator};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThermometerError {
    Unavailable,
    InvalidReading,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeaterError {
    Unavailable,
    CommandFailed,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ControlError {
    Thermometer(ThermometerError),
    Heater(HeaterError),
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
    pub fn new(config: &SimConfig, thermometer: T, heater: H) -> Self {
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
pub struct SimulatedThermometer {
    samples_c: Vec<f32>,
    index: usize,
}

impl SimulatedThermometer {
    pub fn new(samples_c: impl Into<Vec<f32>>) -> Self {
        Self {
            samples_c: samples_c.into(),
            index: 0,
        }
    }

    pub fn from_box_air_simulation(config: SimConfig) -> Self {
        let samples_c = Simulator::new(config)
            .run()
            .into_iter()
            .map(|state| state.box_air_temp_c)
            .collect::<Vec<_>>();

        Self::new(samples_c)
    }

    pub fn from_tempeh_core_simulation(config: SimConfig) -> Self {
        let samples_c = Simulator::new(config)
            .run()
            .into_iter()
            .map(|state| state.tempeh_core_temp_c)
            .collect::<Vec<_>>();

        Self::new(samples_c)
    }

    pub fn len(&self) -> usize {
        self.samples_c.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples_c.is_empty()
    }
}

impl Thermometer for SimulatedThermometer {
    fn read_celsius(&mut self) -> Result<f32, ThermometerError> {
        if self.samples_c.is_empty() {
            return Err(ThermometerError::Unavailable);
        }

        let index = self.index.min(self.samples_c.len() - 1);
        let reading = self.samples_c[index];
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
pub struct SimulatedControlRun {
    pub readings: Vec<ControlReading>,
    pub heater_commands: Vec<bool>,
}

pub fn run_simulated_control(config: SimConfig) -> Result<SimulatedControlRun, ControlError> {
    let thermometer = SimulatedThermometer::from_box_air_simulation(config);
    let heater = LoggingHeater::new();
    let mut control = ControlLoop::new(&config, thermometer, heater);

    let mut readings = Vec::new();
    let mut time_s = 0.0;

    while time_s <= config.duration_s {
        readings.push(control.step(time_s)?);
        time_s += config.dt_s;
    }

    let heater_commands = control.heater().commands().to_vec();

    Ok(SimulatedControlRun {
        readings,
        heater_commands,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulated_thermometer_returns_samples_in_order() {
        let mut thermometer = SimulatedThermometer::new(vec![20.0, 21.5, 22.0]);

        assert_eq!(thermometer.read_celsius().unwrap(), 20.0);
        assert_eq!(thermometer.read_celsius().unwrap(), 21.5);
        assert_eq!(thermometer.read_celsius().unwrap(), 22.0);
    }

    #[test]
    fn simulated_thermometer_holds_last_sample() {
        let mut thermometer = SimulatedThermometer::new(vec![20.0, 21.0]);

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
    fn control_loop_turns_heater_on_when_cold() {
        let config = SimConfig::default();
        let thermometer = SimulatedThermometer::new(vec![20.0]);
        let heater = LoggingHeater::new();
        let mut control = ControlLoop::new(&config, thermometer, heater);

        let reading = control.step(0.0).unwrap();

        assert!(reading.heater_on);
        assert!(control.heater().heater_on());
    }

    #[test]
    fn control_loop_turns_heater_off_at_hard_cutoff() {
        let config = SimConfig::default();
        let thermometer = SimulatedThermometer::new(vec![20.0, config.hard_box_cutoff_c + 0.5]);
        let heater = LoggingHeater::new();
        let mut control = ControlLoop::new(&config, thermometer, heater);

        let first = control.step(0.0).unwrap();
        let second = control.step(config.dt_s).unwrap();

        assert!(first.heater_on);
        assert!(!second.heater_on);
        assert!(!control.heater().heater_on());
    }

    #[test]
    fn simulated_control_run_produces_readings_and_commands() {
        let config = SimConfig {
            duration_s: 60.0,
            dt_s: 10.0,
            ..SimConfig::default()
        };

        let run = run_simulated_control(config).unwrap();

        assert!(!run.readings.is_empty());
        assert_eq!(run.readings.len(), run.heater_commands.len());
    }
}
