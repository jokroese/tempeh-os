use tempeh_control::Controller;
use tempeh_model::{ControllerConfig, TemperatureProbe};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RealRunConfig {
    pub controller: ControllerConfig,
    pub box_air_hard_cutoff_c: f32,
    pub product_hard_cutoff_c: f32,
}

impl Default for RealRunConfig {
    fn default() -> Self {
        Self {
            controller: ControllerConfig::default(),
            box_air_hard_cutoff_c: 34.0,
            product_hard_cutoff_c: 34.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProbeSnapshot {
    pub box_air_temp_c: f32,
    pub room_air_temp_c: Option<f32>,
    pub product_temp_c: Option<f32>,
    pub updated_probe: TemperatureProbe,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeaterDecision {
    pub heater_on: bool,
    pub reason: &'static str,
}

#[derive(Debug, Clone)]
pub struct RealRunController {
    config: RealRunConfig,
    controller: Controller,
    heater_on: bool,
}

impl RealRunController {
    pub fn new(config: RealRunConfig) -> Self {
        Self {
            controller: Controller::new(config.controller),
            config,
            heater_on: false,
        }
    }

    pub fn update(&mut self, snapshot: ProbeSnapshot) -> HeaterDecision {
        if snapshot.box_air_temp_c >= self.config.box_air_hard_cutoff_c {
            self.heater_on = false;
            return HeaterDecision {
                heater_on: false,
                reason: "box_air_hard_cutoff",
            };
        }
        if snapshot
            .product_temp_c
            .is_some_and(|temp| temp >= self.config.product_hard_cutoff_c)
        {
            self.heater_on = false;
            return HeaterDecision {
                heater_on: false,
                reason: "product_hard_cutoff",
            };
        }
        match snapshot.updated_probe {
            TemperatureProbe::BoxAir => {
                let previous = self.heater_on;
                self.heater_on = self.controller.update(snapshot.box_air_temp_c);
                HeaterDecision {
                    heater_on: self.heater_on,
                    reason: control_reason(self.heater_on, previous),
                }
            }
            TemperatureProbe::RoomAir => HeaterDecision {
                heater_on: self.heater_on,
                reason: "room_update",
            },
            TemperatureProbe::Product => HeaterDecision {
                heater_on: self.heater_on,
                reason: "product_update",
            },
        }
    }

    pub fn heater_on(&self) -> bool {
        self.heater_on
    }
}

fn control_reason(heater_on: bool, previous_heater_on: bool) -> &'static str {
    if heater_on && !previous_heater_on {
        "below_target"
    } else if !heater_on && previous_heater_on {
        "above_target"
    } else if heater_on {
        "holding_on"
    } else {
        "holding_off"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_heater_on_when_box_air_is_below_target_band() {
        let mut controller = RealRunController::new(RealRunConfig::default());
        let decision = controller.update(ProbeSnapshot {
            box_air_temp_c: 20.0,
            room_air_temp_c: Some(20.0),
            product_temp_c: Some(20.0),
            updated_probe: TemperatureProbe::BoxAir,
        });
        assert!(decision.heater_on);
        assert_eq!(decision.reason, "below_target");
    }

    #[test]
    fn turns_heater_off_on_box_air_hard_cutoff() {
        let mut controller = RealRunController::new(RealRunConfig::default());
        let decision = controller.update(ProbeSnapshot {
            box_air_temp_c: 34.0,
            room_air_temp_c: Some(20.0),
            product_temp_c: Some(30.0),
            updated_probe: TemperatureProbe::BoxAir,
        });
        assert!(!decision.heater_on);
        assert_eq!(decision.reason, "box_air_hard_cutoff");
    }

    #[test]
    fn turns_heater_off_on_product_hard_cutoff() {
        let mut controller = RealRunController::new(RealRunConfig::default());
        let decision = controller.update(ProbeSnapshot {
            box_air_temp_c: 30.0,
            room_air_temp_c: Some(20.0),
            product_temp_c: Some(34.0),
            updated_probe: TemperatureProbe::Product,
        });
        assert!(!decision.heater_on);
        assert_eq!(decision.reason, "product_hard_cutoff");
    }

    #[test]
    fn room_update_does_not_run_hysteresis_controller() {
        let mut controller = RealRunController::new(RealRunConfig::default());
        let first = controller.update(ProbeSnapshot {
            box_air_temp_c: 20.0,
            room_air_temp_c: Some(20.0),
            product_temp_c: Some(20.0),
            updated_probe: TemperatureProbe::BoxAir,
        });
        assert!(first.heater_on);
        let second = controller.update(ProbeSnapshot {
            box_air_temp_c: 30.5,
            room_air_temp_c: Some(19.0),
            product_temp_c: Some(20.0),
            updated_probe: TemperatureProbe::RoomAir,
        });
        assert!(second.heater_on);
        assert_eq!(second.reason, "room_update");
    }

    #[test]
    fn product_update_can_trigger_safety_but_not_normal_hysteresis() {
        let mut controller = RealRunController::new(RealRunConfig::default());
        let first = controller.update(ProbeSnapshot {
            box_air_temp_c: 20.0,
            room_air_temp_c: Some(20.0),
            product_temp_c: Some(20.0),
            updated_probe: TemperatureProbe::BoxAir,
        });
        assert!(first.heater_on);
        let second = controller.update(ProbeSnapshot {
            box_air_temp_c: 31.0,
            room_air_temp_c: Some(20.0),
            product_temp_c: Some(31.0),
            updated_probe: TemperatureProbe::Product,
        });
        assert!(second.heater_on);
        assert_eq!(second.reason, "product_update");
    }
}
