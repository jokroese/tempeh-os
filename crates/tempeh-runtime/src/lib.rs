use tempeh_control::Controller;
use tempeh_model::{ControllerConfig, TemperatureProbe, TemperatureReading};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RealRunConfig {
    pub controller: ControllerConfig,
    pub box_air_hard_cutoff_c: f32,
    pub product_hard_cutoff_c: f32,
    pub max_box_air_age_s: f32,
    pub max_product_age_s: f32,
}

impl Default for RealRunConfig {
    fn default() -> Self {
        Self {
            controller: ControllerConfig::default(),
            box_air_hard_cutoff_c: 34.0,
            product_hard_cutoff_c: 34.0,
            max_box_air_age_s: 20.0,
            max_product_age_s: 60.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatestTemperatureReadings {
    pub room_air_temp_c: Option<f32>,
    pub box_air_temp_c: Option<f32>,
    pub product_temp_c: Option<f32>,
    pub room_air_updated_at_s: Option<f32>,
    pub box_air_updated_at_s: Option<f32>,
    pub product_updated_at_s: Option<f32>,
}

impl LatestTemperatureReadings {
    pub fn new() -> Self {
        Self {
            room_air_temp_c: None,
            box_air_temp_c: None,
            product_temp_c: None,
            room_air_updated_at_s: None,
            box_air_updated_at_s: None,
            product_updated_at_s: None,
        }
    }

    pub fn update(&mut self, probe: TemperatureProbe, temp_c: f32) {
        self.update_at(0.0, probe, temp_c);
    }

    pub fn update_at(&mut self, time_s: f32, probe: TemperatureProbe, temp_c: f32) {
        match probe {
            TemperatureProbe::RoomAir => {
                self.room_air_temp_c = Some(temp_c);
                self.room_air_updated_at_s = Some(time_s);
            }
            TemperatureProbe::BoxAir => {
                self.box_air_temp_c = Some(temp_c);
                self.box_air_updated_at_s = Some(time_s);
            }
            TemperatureProbe::Product => {
                self.product_temp_c = Some(temp_c);
                self.product_updated_at_s = Some(time_s);
            }
        }
    }

    pub fn reading(&self, time_s: f32) -> Option<TemperatureReading> {
        Some(TemperatureReading {
            time_s,
            room_air_temp_c: self.room_air_temp_c,
            box_air_temp_c: self.box_air_temp_c?,
            product_temp_c: self.product_temp_c,
        })
    }

    pub fn snapshot_for_update(&self, updated_probe: TemperatureProbe) -> Option<ProbeSnapshot> {
        self.snapshot_for_update_at(0.0, updated_probe)
    }

    pub fn snapshot_for_update_at(
        &self,
        time_s: f32,
        updated_probe: TemperatureProbe,
    ) -> Option<ProbeSnapshot> {
        let box_air_updated_at_s = self.box_air_updated_at_s?;

        Some(ProbeSnapshot {
            box_air_temp_c: self.box_air_temp_c?,
            room_air_temp_c: self.room_air_temp_c,
            product_temp_c: self.product_temp_c,
            box_air_age_s: time_s - box_air_updated_at_s,
            product_age_s: self
                .product_updated_at_s
                .map(|updated_at| time_s - updated_at),
            updated_probe,
        })
    }
}

impl Default for LatestTemperatureReadings {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProbeSnapshot {
    pub box_air_temp_c: f32,
    pub room_air_temp_c: Option<f32>,
    pub product_temp_c: Option<f32>,
    pub box_air_age_s: f32,
    pub product_age_s: Option<f32>,
    pub updated_probe: TemperatureProbe,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RealRunSample {
    pub time_s: f32,
    pub room_air_temp_c: Option<f32>,
    pub box_air_temp_c: f32,
    pub product_temp_c: Option<f32>,
    pub heater_on: bool,
    pub reason: &'static str,
}

impl RealRunSample {
    pub fn csv_header() -> &'static str {
        "time_s,room_air_temp_c,box_air_temp_c,product_temp_c,heater_on,reason"
    }

    pub fn csv_row(&self) -> String {
        let room_air_text = self
            .room_air_temp_c
            .map(|temp| format!("{temp:.3}"))
            .unwrap_or_default();
        let product_text = self
            .product_temp_c
            .map(|temp| format!("{temp:.3}"))
            .unwrap_or_default();

        format!(
            "{:.0},{},{:.3},{},{},{}",
            self.time_s,
            room_air_text,
            self.box_air_temp_c,
            product_text,
            if self.heater_on { 1 } else { 0 },
            self.reason,
        )
    }
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
        if snapshot.box_air_age_s > self.config.max_box_air_age_s {
            self.heater_on = false;
            return HeaterDecision {
                heater_on: false,
                reason: "box_air_stale",
            };
        }

        if snapshot
            .product_age_s
            .is_some_and(|age_s| age_s > self.config.max_product_age_s)
        {
            self.heater_on = false;
            return HeaterDecision {
                heater_on: false,
                reason: "product_stale",
            };
        }

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

    pub fn update_sample(
        &mut self,
        time_s: f32,
        latest: &LatestTemperatureReadings,
        updated_probe: TemperatureProbe,
    ) -> Option<RealRunSample> {
        let snapshot = latest.snapshot_for_update_at(time_s, updated_probe)?;
        let decision = self.update(snapshot);

        Some(RealRunSample {
            time_s,
            room_air_temp_c: latest.room_air_temp_c,
            box_air_temp_c: latest.box_air_temp_c?,
            product_temp_c: latest.product_temp_c,
            heater_on: decision.heater_on,
            reason: decision.reason,
        })
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
    fn latest_temperature_readings_start_empty() {
        let latest = LatestTemperatureReadings::new();

        assert_eq!(latest.room_air_temp_c, None);
        assert_eq!(latest.box_air_temp_c, None);
        assert_eq!(latest.product_temp_c, None);
        assert_eq!(latest.room_air_updated_at_s, None);
        assert_eq!(latest.box_air_updated_at_s, None);
        assert_eq!(latest.product_updated_at_s, None);
        assert_eq!(latest.reading(0.0), None);
        assert_eq!(latest.snapshot_for_update(TemperatureProbe::BoxAir), None);
    }

    #[test]
    fn latest_temperature_readings_update_by_probe() {
        let mut latest = LatestTemperatureReadings::new();

        latest.update(TemperatureProbe::RoomAir, 20.2);
        latest.update(TemperatureProbe::BoxAir, 22.4);
        latest.update(TemperatureProbe::Product, 23.1);

        assert_eq!(latest.room_air_temp_c, Some(20.2));
        assert_eq!(latest.box_air_temp_c, Some(22.4));
        assert_eq!(latest.product_temp_c, Some(23.1));
        assert_eq!(latest.room_air_updated_at_s, Some(0.0));
        assert_eq!(latest.box_air_updated_at_s, Some(0.0));
        assert_eq!(latest.product_updated_at_s, Some(0.0));
    }

    #[test]
    fn latest_temperature_readings_emit_model_reading_once_box_air_exists() {
        let mut latest = LatestTemperatureReadings::new();

        latest.update(TemperatureProbe::RoomAir, 20.2);
        assert_eq!(latest.reading(1.0), None);

        latest.update(TemperatureProbe::BoxAir, 22.4);

        assert_eq!(
            latest.reading(2.0),
            Some(TemperatureReading {
                time_s: 2.0,
                room_air_temp_c: Some(20.2),
                box_air_temp_c: 22.4,
                product_temp_c: None,
            })
        );
    }

    #[test]
    fn latest_temperature_readings_emit_probe_snapshot_once_box_air_exists() {
        let mut latest = LatestTemperatureReadings::new();

        latest.update(TemperatureProbe::Product, 23.1);
        assert_eq!(latest.snapshot_for_update(TemperatureProbe::Product), None);

        latest.update(TemperatureProbe::BoxAir, 22.4);

        assert_eq!(
            latest.snapshot_for_update(TemperatureProbe::Product),
            Some(ProbeSnapshot {
                box_air_temp_c: 22.4,
                room_air_temp_c: None,
                product_temp_c: Some(23.1),
                box_air_age_s: 0.0,
                product_age_s: Some(0.0),
                updated_probe: TemperatureProbe::Product,
            })
        );
    }

    #[test]
    fn latest_temperature_readings_emit_probe_snapshot_with_ages() {
        let mut latest = LatestTemperatureReadings::new();

        latest.update_at(10.0, TemperatureProbe::BoxAir, 22.4);
        latest.update_at(20.0, TemperatureProbe::Product, 23.1);

        assert_eq!(
            latest.snapshot_for_update_at(25.0, TemperatureProbe::RoomAir),
            Some(ProbeSnapshot {
                box_air_temp_c: 22.4,
                room_air_temp_c: None,
                product_temp_c: Some(23.1),
                box_air_age_s: 15.0,
                product_age_s: Some(5.0),
                updated_probe: TemperatureProbe::RoomAir,
            })
        );
    }

    #[test]
    fn latest_temperature_readings_do_not_report_product_age_before_product_seen() {
        let mut latest = LatestTemperatureReadings::new();

        latest.update_at(10.0, TemperatureProbe::BoxAir, 22.4);

        assert_eq!(
            latest
                .snapshot_for_update_at(25.0, TemperatureProbe::BoxAir)
                .unwrap()
                .product_age_s,
            None
        );
    }

    #[test]
    fn latest_temperature_readings_do_not_require_room_air_for_snapshot() {
        let mut latest = LatestTemperatureReadings::new();

        latest.update_at(10.0, TemperatureProbe::BoxAir, 22.4);
        latest.update_at(12.0, TemperatureProbe::Product, 23.1);

        assert_eq!(
            latest.snapshot_for_update_at(15.0, TemperatureProbe::Product),
            Some(ProbeSnapshot {
                box_air_temp_c: 22.4,
                room_air_temp_c: None,
                product_temp_c: Some(23.1),
                box_air_age_s: 5.0,
                product_age_s: Some(3.0),
                updated_probe: TemperatureProbe::Product,
            })
        );
    }

    #[test]
    fn real_run_sample_writes_csv_header() {
        assert_eq!(
            RealRunSample::csv_header(),
            "time_s,room_air_temp_c,box_air_temp_c,product_temp_c,heater_on,reason"
        );
    }

    #[test]
    fn real_run_sample_writes_csv_row_with_all_temperatures() {
        let sample = RealRunSample {
            time_s: 2.0,
            room_air_temp_c: Some(20.2),
            box_air_temp_c: 22.4,
            product_temp_c: Some(23.1),
            heater_on: true,
            reason: "below_target",
        };

        assert_eq!(sample.csv_row(), "2,20.200,22.400,23.100,1,below_target");
    }

    #[test]
    fn real_run_sample_writes_csv_row_without_optional_temperatures() {
        let sample = RealRunSample {
            time_s: 2.0,
            room_air_temp_c: None,
            box_air_temp_c: 22.4,
            product_temp_c: None,
            heater_on: false,
            reason: "holding_off",
        };

        assert_eq!(sample.csv_row(), "2,,22.400,,0,holding_off");
    }

    #[test]
    fn real_run_controller_emits_sample_from_latest_readings() {
        let mut latest = LatestTemperatureReadings::new();
        latest.update_at(10.0, TemperatureProbe::BoxAir, 20.0);
        latest.update_at(12.0, TemperatureProbe::Product, 21.0);

        let mut controller = RealRunController::new(RealRunConfig::default());
        let sample = controller
            .update_sample(12.0, &latest, TemperatureProbe::Product)
            .expect("real run sample");

        assert_eq!(
            sample,
            RealRunSample {
                time_s: 12.0,
                room_air_temp_c: None,
                box_air_temp_c: 20.0,
                product_temp_c: Some(21.0),
                heater_on: false,
                reason: "product_update",
            }
        );
    }

    #[test]
    fn real_run_controller_returns_no_sample_without_box_air() {
        let mut latest = LatestTemperatureReadings::new();
        latest.update_at(10.0, TemperatureProbe::Product, 21.0);

        let mut controller = RealRunController::new(RealRunConfig::default());

        assert_eq!(
            controller.update_sample(10.0, &latest, TemperatureProbe::Product),
            None
        );
    }

    #[test]
    fn turns_heater_on_when_box_air_is_below_target_band() {
        let mut controller = RealRunController::new(RealRunConfig::default());
        let decision = controller.update(ProbeSnapshot {
            box_air_temp_c: 20.0,
            room_air_temp_c: Some(20.0),
            product_temp_c: Some(20.0),
            box_air_age_s: 0.0,
            product_age_s: Some(0.0),
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
            box_air_age_s: 0.0,
            product_age_s: Some(0.0),
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
            box_air_age_s: 0.0,
            product_age_s: Some(0.0),
            updated_probe: TemperatureProbe::Product,
        });
        assert!(!decision.heater_on);
        assert_eq!(decision.reason, "product_hard_cutoff");
    }

    #[test]
    fn turns_heater_off_when_box_air_is_stale() {
        let mut controller = RealRunController::new(RealRunConfig::default());

        let decision = controller.update(ProbeSnapshot {
            box_air_temp_c: 30.0,
            room_air_temp_c: Some(20.0),
            product_temp_c: Some(30.0),
            box_air_age_s: RealRunConfig::default().max_box_air_age_s + 0.1,
            product_age_s: Some(0.0),
            updated_probe: TemperatureProbe::RoomAir,
        });

        assert!(!decision.heater_on);
        assert_eq!(decision.reason, "box_air_stale");
    }

    #[test]
    fn turns_heater_off_when_seen_product_is_stale() {
        let mut controller = RealRunController::new(RealRunConfig::default());

        let decision = controller.update(ProbeSnapshot {
            box_air_temp_c: 30.0,
            room_air_temp_c: Some(20.0),
            product_temp_c: Some(30.0),
            box_air_age_s: 0.0,
            product_age_s: Some(RealRunConfig::default().max_product_age_s + 0.1),
            updated_probe: TemperatureProbe::RoomAir,
        });

        assert!(!decision.heater_on);
        assert_eq!(decision.reason, "product_stale");
    }

    #[test]
    fn missing_product_age_does_not_force_heater_off() {
        let mut controller = RealRunController::new(RealRunConfig::default());

        let decision = controller.update(ProbeSnapshot {
            box_air_temp_c: 20.0,
            room_air_temp_c: Some(20.0),
            product_temp_c: None,
            box_air_age_s: 0.0,
            product_age_s: None,
            updated_probe: TemperatureProbe::BoxAir,
        });

        assert!(decision.heater_on);
        assert_eq!(decision.reason, "below_target");
    }

    #[test]
    fn missing_room_air_does_not_force_heater_off() {
        let mut controller = RealRunController::new(RealRunConfig::default());

        let decision = controller.update(ProbeSnapshot {
            box_air_temp_c: 20.0,
            room_air_temp_c: None,
            product_temp_c: Some(20.0),
            box_air_age_s: 0.0,
            product_age_s: Some(0.0),
            updated_probe: TemperatureProbe::BoxAir,
        });

        assert!(decision.heater_on);
        assert_eq!(decision.reason, "below_target");
    }

    #[test]
    fn room_update_does_not_run_hysteresis_controller() {
        let mut controller = RealRunController::new(RealRunConfig::default());
        let first = controller.update(ProbeSnapshot {
            box_air_temp_c: 20.0,
            room_air_temp_c: Some(20.0),
            product_temp_c: Some(20.0),
            box_air_age_s: 0.0,
            product_age_s: Some(0.0),
            updated_probe: TemperatureProbe::BoxAir,
        });
        assert!(first.heater_on);
        let second = controller.update(ProbeSnapshot {
            box_air_temp_c: 30.5,
            room_air_temp_c: Some(19.0),
            product_temp_c: Some(20.0),
            box_air_age_s: 0.0,
            product_age_s: Some(0.0),
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
            box_air_age_s: 0.0,
            product_age_s: Some(0.0),
            updated_probe: TemperatureProbe::BoxAir,
        });
        assert!(first.heater_on);
        let second = controller.update(ProbeSnapshot {
            box_air_temp_c: 31.0,
            room_air_temp_c: Some(20.0),
            product_temp_c: Some(31.0),
            box_air_age_s: 0.0,
            product_age_s: Some(0.0),
            updated_probe: TemperatureProbe::Product,
        });
        assert!(second.heater_on);
        assert_eq!(second.reason, "product_update");
    }
}
