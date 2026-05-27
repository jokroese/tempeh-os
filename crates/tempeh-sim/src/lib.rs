use tempeh_model::{ControllerConfig, EnvironmentState};

#[derive(Debug, Clone, Copy)]
pub struct SimConfig {
    pub dt_s: f32,
    pub duration_s: f32,
    pub initial_room_air_temp_c: f32,
    pub initial_box_air_temp_c: f32,
    pub initial_tempeh_core_temp_c: f32,
    pub controller: ControllerConfig,
    pub heater_gain_c_per_s: f32,
    pub box_loss_rate_per_s: f32,
    pub air_to_tempeh_rate_per_s: f32,
    pub tempeh_to_air_rate_per_s: f32,
    pub base_growth_rate_per_s: f32,
    pub max_metabolic_heat_c_per_s: f32,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            dt_s: 10.0,
            duration_s: 48.0 * 60.0 * 60.0,
            initial_room_air_temp_c: 20.0,
            initial_box_air_temp_c: 20.0,
            initial_tempeh_core_temp_c: 25.0,
            controller: ControllerConfig::default(),
            heater_gain_c_per_s: 0.006,
            box_loss_rate_per_s: 0.00018,
            air_to_tempeh_rate_per_s: 0.00010,
            tempeh_to_air_rate_per_s: 0.00004,
            base_growth_rate_per_s: 1.0 / (34.0 * 60.0 * 60.0),
            max_metabolic_heat_c_per_s: 0.00016,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Simulator {
    config: SimConfig,
    state: EnvironmentState,
}

impl Simulator {
    pub fn new(config: SimConfig) -> Self {
        let state = EnvironmentState {
            time_s: 0.0,
            room_air_temp_c: config.initial_room_air_temp_c,
            box_air_temp_c: config.initial_box_air_temp_c,
            tempeh_core_temp_c: config.initial_tempeh_core_temp_c,
            fermentation_progress: 0.0,
            metabolic_heat_rate_c_per_s: 0.0,
            heater_on: false,
        };
        Self { config, state }
    }

    pub fn state(&self) -> EnvironmentState {
        self.state
    }

    pub fn step(&mut self, heater_on: bool) -> EnvironmentState {
        let dt_s = self.config.dt_s;
        let suitability = temp_suitability(self.state.tempeh_core_temp_c);
        let growth_delta = self.config.base_growth_rate_per_s * suitability * dt_s;
        let fermentation_progress =
            (self.state.fermentation_progress + growth_delta).clamp(0.0, 1.0);
        let metabolic_heat_rate_c_per_s = metabolic_heat_rate_c_per_s(
            fermentation_progress,
            self.config.max_metabolic_heat_c_per_s,
        );
        let heater_gain = if heater_on {
            self.config.heater_gain_c_per_s * dt_s
        } else {
            0.0
        };
        let box_loss = (self.state.room_air_temp_c - self.state.box_air_temp_c)
            * self.config.box_loss_rate_per_s
            * dt_s;
        let tempeh_to_air = (self.state.tempeh_core_temp_c - self.state.box_air_temp_c)
            * self.config.tempeh_to_air_rate_per_s
            * dt_s;
        let box_air_temp_c = self.state.box_air_temp_c + heater_gain + box_loss + tempeh_to_air;
        let air_to_tempeh = (box_air_temp_c - self.state.tempeh_core_temp_c)
            * self.config.air_to_tempeh_rate_per_s
            * dt_s;
        let metabolic_heat = metabolic_heat_rate_c_per_s * dt_s;
        let tempeh_core_temp_c = self.state.tempeh_core_temp_c + air_to_tempeh + metabolic_heat;

        self.state = EnvironmentState {
            time_s: self.state.time_s + dt_s,
            room_air_temp_c: self.state.room_air_temp_c,
            box_air_temp_c,
            tempeh_core_temp_c,
            fermentation_progress,
            metabolic_heat_rate_c_per_s,
            heater_on,
        };
        self.state
    }
}

pub fn run_open_loop_simulation(config: SimConfig, heater_on: bool) -> Vec<EnvironmentState> {
    let mut simulator = Simulator::new(config);
    let mut samples = vec![simulator.state()];
    while simulator.state().time_s < config.duration_s {
        samples.push(simulator.step(heater_on));
    }
    samples
}

#[derive(Debug, Clone)]
pub struct TemperatureTrace {
    samples_c: Vec<f32>,
}

impl TemperatureTrace {
    pub fn new(samples_c: impl Into<Vec<f32>>) -> Self {
        Self {
            samples_c: samples_c.into(),
        }
    }

    pub fn from_environment_states(
        states: impl IntoIterator<Item = EnvironmentState>,
        value: fn(EnvironmentState) -> f32,
    ) -> Self {
        let samples_c = states.into_iter().map(value).collect::<Vec<_>>();
        Self::new(samples_c)
    }

    pub fn from_box_air_states(states: impl IntoIterator<Item = EnvironmentState>) -> Self {
        Self::from_environment_states(states, |state| state.box_air_temp_c)
    }

    pub fn from_tempeh_core_states(states: impl IntoIterator<Item = EnvironmentState>) -> Self {
        Self::from_environment_states(states, |state| state.tempeh_core_temp_c)
    }

    pub fn sample_or_last(&self, index: usize) -> Option<f32> {
        if self.samples_c.is_empty() {
            None
        } else {
            Some(self.samples_c[index.min(self.samples_c.len() - 1)])
        }
    }

    pub fn len(&self) -> usize {
        self.samples_c.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples_c.is_empty()
    }
}

pub fn temp_suitability(temp_c: f32) -> f32 {
    if temp_c < 24.0 {
        0.0
    } else if temp_c < 28.0 {
        (temp_c - 24.0) / 4.0
    } else if temp_c <= 32.0 {
        1.0
    } else if temp_c <= 36.0 {
        1.0 - ((temp_c - 32.0) / 4.0) * 0.6
    } else {
        0.0
    }
}

pub fn metabolic_heat_rate_c_per_s(progress: f32, max_heat_c_per_s: f32) -> f32 {
    let p = progress.clamp(0.0, 1.0);
    let bell = 4.0 * p * (1.0 - p);
    let early_gate = if p < 0.15 { p / 0.15 } else { 1.0 };
    max_heat_c_per_s * bell * early_gate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_suitability_is_best_in_target_band() {
        assert_eq!(temp_suitability(20.0), 0.0);
        assert!(temp_suitability(26.0) > 0.0);
        assert_eq!(temp_suitability(30.0), 1.0);
        assert!(temp_suitability(34.0) < 1.0);
        assert_eq!(temp_suitability(38.0), 0.0);
    }

    #[test]
    fn metabolic_heat_starts_and_ends_low() {
        let max = 0.001;
        assert!(metabolic_heat_rate_c_per_s(0.0, max) < 0.000001);
        assert!(metabolic_heat_rate_c_per_s(0.5, max) > 0.0009);
        assert!(metabolic_heat_rate_c_per_s(1.0, max) < 0.000001);
    }

    #[test]
    fn simulation_progresses_when_heater_is_on() {
        let config = SimConfig {
            duration_s: 6.0 * 60.0 * 60.0,
            ..SimConfig::default()
        };
        let mut simulator = Simulator::new(config);
        let mut last = simulator.state();
        while last.time_s < config.duration_s {
            last = simulator.step(true);
        }
        assert!(last.time_s >= config.duration_s);
        assert!(last.box_air_temp_c > config.initial_box_air_temp_c);
        assert!(last.fermentation_progress > 0.0);
    }

    #[test]
    fn temperature_trace_holds_last_sample() {
        let trace = TemperatureTrace::new(vec![20.0, 21.0]);
        assert_eq!(trace.sample_or_last(0), Some(20.0));
        assert_eq!(trace.sample_or_last(1), Some(21.0));
        assert_eq!(trace.sample_or_last(2), Some(21.0));
    }

    #[test]
    fn temperature_trace_returns_none_when_empty() {
        let trace = TemperatureTrace::new(Vec::<f32>::new());
        assert_eq!(trace.sample_or_last(0), None);
    }
}
