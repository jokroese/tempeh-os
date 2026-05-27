#[derive(Debug, Clone, Copy)]
pub struct SimConfig {
    pub dt_s: f32,
    pub duration_s: f32,

    pub initial_room_air_temp_c: f32,
    pub initial_box_air_temp_c: f32,
    pub initial_tempeh_core_temp_c: f32,

    pub target_box_air_temp_c: f32,
    pub hysteresis_c: f32,
    pub hard_box_cutoff_c: f32,
    pub hard_tempeh_cutoff_c: f32,

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

            target_box_air_temp_c: 30.0,
            hysteresis_c: 0.4,
            hard_box_cutoff_c: 34.0,
            hard_tempeh_cutoff_c: 37.0,

            // These are intentionally rough v0 model parameters.
            heater_gain_c_per_s: 0.006,
            box_loss_rate_per_s: 0.00018,
            air_to_tempeh_rate_per_s: 0.00010,
            tempeh_to_air_rate_per_s: 0.00004,

            // Reaches near-finished in roughly 30-40 h under good conditions.
            base_growth_rate_per_s: 1.0 / (34.0 * 60.0 * 60.0),

            // Effective self-heating rate of tempeh core.
            // 0.00016 C/s ~= 0.576 C/h at peak.
            max_metabolic_heat_c_per_s: 0.00016,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SimState {
    pub time_s: f32,
    pub room_air_temp_c: f32,
    pub box_air_temp_c: f32,
    pub tempeh_core_temp_c: f32,
    pub fermentation_progress: f32,
    pub metabolic_heat_rate_c_per_s: f32,
    pub heater_on: bool,
}

impl SimState {
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

#[derive(Debug, Clone, Copy)]
pub struct Controller {
    pub target_box_air_temp_c: f32,
    pub hysteresis_c: f32,
    pub hard_box_cutoff_c: f32,
    pub hard_tempeh_cutoff_c: f32,
    heater_on: bool,
}

impl Controller {
    pub fn new(config: &SimConfig) -> Self {
        Self {
            target_box_air_temp_c: config.target_box_air_temp_c,
            hysteresis_c: config.hysteresis_c,
            hard_box_cutoff_c: config.hard_box_cutoff_c,
            hard_tempeh_cutoff_c: config.hard_tempeh_cutoff_c,
            heater_on: false,
        }
    }

    pub fn update(&mut self, box_air_temp_c: f32, tempeh_core_temp_c: f32) -> bool {
        if box_air_temp_c >= self.hard_box_cutoff_c
            || tempeh_core_temp_c >= self.hard_tempeh_cutoff_c
        {
            self.heater_on = false;
            return self.heater_on;
        }

        if !self.heater_on && box_air_temp_c < self.target_box_air_temp_c - self.hysteresis_c {
            self.heater_on = true;
        }

        if self.heater_on && box_air_temp_c > self.target_box_air_temp_c + self.hysteresis_c {
            self.heater_on = false;
        }

        self.heater_on
    }
}

#[derive(Debug, Clone)]
pub struct Simulator {
    config: SimConfig,
    controller: Controller,
    state: SimState,
}

impl Simulator {
    pub fn new(config: SimConfig) -> Self {
        let controller = Controller::new(&config);
        let state = SimState {
            time_s: 0.0,
            room_air_temp_c: config.initial_room_air_temp_c,
            box_air_temp_c: config.initial_box_air_temp_c,
            tempeh_core_temp_c: config.initial_tempeh_core_temp_c,
            fermentation_progress: 0.0,
            metabolic_heat_rate_c_per_s: 0.0,
            heater_on: false,
        };

        Self {
            config,
            controller,
            state,
        }
    }

    pub fn state(&self) -> SimState {
        self.state
    }

    pub fn step(&mut self) -> SimState {
        let dt_s = self.config.dt_s;

        let heater_on = self
            .controller
            .update(self.state.box_air_temp_c, self.state.tempeh_core_temp_c);

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

        self.state = SimState {
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

    pub fn run(&mut self) -> Vec<SimState> {
        let mut samples = Vec::new();
        samples.push(self.state());

        while self.state.time_s < self.config.duration_s {
            samples.push(self.step());
        }

        samples
    }
}

pub fn run_simulation(config: SimConfig) -> Vec<SimState> {
    Simulator::new(config).run()
}

/// Returns 0..1 suitability for Rhizopus-ish growth in this toy model.
/// This is an operational model, not a biological truth.
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

/// Effective tempeh self-heating rate in °C/s.
///
/// The curve is near-zero very early, peaks in mid/late fermentation,
/// and tapers as progress approaches completion.
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
    fn controller_turns_off_at_hard_cutoff() {
        let config = SimConfig::default();
        let mut controller = Controller::new(&config);

        assert!(controller.update(20.0, 20.0));
        assert!(!controller.update(config.hard_box_cutoff_c + 0.1, 20.0));
        assert!(!controller.update(20.0, config.hard_tempeh_cutoff_c + 0.1));
    }

    #[test]
    fn simulation_progresses() {
        let config = SimConfig {
            duration_s: 6.0 * 60.0 * 60.0,
            ..SimConfig::default()
        };

        let samples = run_simulation(config);
        let last = samples.last().unwrap();

        assert!(last.time_s >= config.duration_s);
        assert!(last.box_air_temp_c > config.initial_box_air_temp_c);
        assert!(last.fermentation_progress > 0.0);
    }
}
