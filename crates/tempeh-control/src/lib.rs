use tempeh_model::ControllerConfig;

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

    pub fn update(&mut self, box_air_temp_c: f32) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_uses_hysteresis() {
        let config = ControllerConfig::default();
        let mut controller = Controller::new(config);

        assert!(controller.update(20.0));
        assert!(controller.update(config.target_box_air_temp_c));
        assert!(!controller.update(config.target_box_air_temp_c + config.hysteresis_c + 0.1));
        assert!(!controller.update(config.target_box_air_temp_c));
        assert!(controller.update(config.target_box_air_temp_c - config.hysteresis_c - 0.1));
    }
}
