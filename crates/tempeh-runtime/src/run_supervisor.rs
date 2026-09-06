use tempeh_model::TemperatureProbe;

use crate::{
    LatestTemperatureReadings, RealRunConfig, RealRunController, RealRunSample, RealRunUpdate,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultReason {
    BoxAirStale,
    ProductStale,
    BoxAirHardCutoff,
    ProductHardCutoff,
    ActuatorFailed,
    BootConfigFailed,
}

impl FaultReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BoxAirStale => "box_air_stale",
            Self::ProductStale => "product_stale",
            Self::BoxAirHardCutoff => "box_air_hard_cutoff",
            Self::ProductHardCutoff => "product_hard_cutoff",
            Self::ActuatorFailed => "actuator_failed",
            Self::BootConfigFailed => "boot_config_failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Idle,
    Running,
    Fault(FaultReason),
}

impl RunState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Fault(_) => "fault",
        }
    }

    pub fn is_running(self) -> bool {
        matches!(self, Self::Running)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunCommand {
    Start,
    Stop,
    AcknowledgeFault,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SupervisorConfig {
    pub run: RealRunConfig,
    pub product_required: bool,
}

impl SupervisorConfig {
    pub fn new(run: RealRunConfig, product_required: bool) -> Self {
        Self {
            run,
            product_required,
        }
    }
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self::new(RealRunConfig::default(), true)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SupervisorOutcome {
    pub state: RunState,
    pub desired_heater_on: bool,
    pub sample: Option<RealRunSample>,
    pub state_change: Option<StateChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateChange {
    pub state: RunState,
    pub reason: &'static str,
}

#[derive(Debug, Clone)]
pub struct RunSupervisor {
    config: SupervisorConfig,
    state: RunState,
    controller: RealRunController,
}

impl RunSupervisor {
    pub fn new(config: SupervisorConfig) -> Self {
        Self {
            controller: RealRunController::new(config.run),
            config,
            state: RunState::Idle,
        }
    }

    pub fn state(&self) -> RunState {
        self.state
    }

    pub fn desired_heater_on(&self) -> bool {
        self.state.is_running() && self.controller.heater_on()
    }

    pub fn handle_command(
        &mut self,
        time_s: f32,
        latest: &LatestTemperatureReadings,
        command: RunCommand,
    ) -> SupervisorOutcome {
        match (self.state, command) {
            (RunState::Idle, RunCommand::Start) => match self.probes_ready(time_s, latest) {
                Ok(()) => self.enter(RunState::Running, "user_start"),
                Err(_) => self.unchanged(),
            },
            (RunState::Running, RunCommand::Stop) => self.enter(RunState::Idle, "user_stop"),
            (RunState::Fault(_), RunCommand::AcknowledgeFault) => {
                match self.probes_ready(time_s, latest) {
                    Ok(()) => self.enter(RunState::Idle, "fault_acknowledged"),
                    Err(_) => self.unchanged(),
                }
            }
            _ => self.unchanged(),
        }
    }

    pub fn tick(
        &mut self,
        time_s: f32,
        latest: &LatestTemperatureReadings,
        update: RealRunUpdate,
    ) -> SupervisorOutcome {
        if !self.state.is_running() {
            return self.unchanged();
        }

        if let Err(reason) = self.probes_ready(time_s, latest) {
            return self.enter(RunState::Fault(reason), reason.as_str());
        }

        let sample = self.controller.evaluate_sample(time_s, latest, update);

        SupervisorOutcome {
            state: self.state,
            desired_heater_on: self.desired_heater_on(),
            sample,
            state_change: None,
        }
    }

    pub fn report_fault(&mut self, reason: FaultReason) -> SupervisorOutcome {
        if matches!(self.state, RunState::Fault(_)) {
            return self.unchanged();
        }
        self.enter(RunState::Fault(reason), reason.as_str())
    }

    fn probes_ready(
        &self,
        time_s: f32,
        latest: &LatestTemperatureReadings,
    ) -> Result<(), FaultReason> {
        let snapshot = latest
            .snapshot_for_update_at(time_s, TemperatureProbe::BoxAir)
            .ok_or(FaultReason::BoxAirStale)?;

        if snapshot.box_air_age_s > self.config.run.max_box_air_age_s {
            return Err(FaultReason::BoxAirStale);
        }

        if self.config.product_required {
            let product_age_s = snapshot.product_age_s.ok_or(FaultReason::ProductStale)?;
            if product_age_s > self.config.run.max_product_age_s {
                return Err(FaultReason::ProductStale);
            }
        } else if snapshot
            .product_age_s
            .is_some_and(|age_s| age_s > self.config.run.max_product_age_s)
        {
            return Err(FaultReason::ProductStale);
        }

        if snapshot.box_air_temp_c >= self.config.run.box_air_hard_cutoff_c {
            return Err(FaultReason::BoxAirHardCutoff);
        }

        if snapshot
            .product_temp_c
            .is_some_and(|temp_c| temp_c >= self.config.run.product_hard_cutoff_c)
        {
            return Err(FaultReason::ProductHardCutoff);
        }

        Ok(())
    }

    fn enter(&mut self, state: RunState, reason: &'static str) -> SupervisorOutcome {
        self.state = state;
        self.controller = RealRunController::new(self.config.run);

        SupervisorOutcome {
            state,
            desired_heater_on: self.desired_heater_on(),
            sample: None,
            state_change: Some(StateChange { state, reason }),
        }
    }

    fn unchanged(&self) -> SupervisorOutcome {
        SupervisorOutcome {
            state: self.state,
            desired_heater_on: self.desired_heater_on(),
            sample: None,
            state_change: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_readings(time_s: f32, box_air_temp_c: f32) -> LatestTemperatureReadings {
        let mut latest = LatestTemperatureReadings::new();
        latest.update_at(time_s, TemperatureProbe::BoxAir, box_air_temp_c);
        latest.update_at(time_s, TemperatureProbe::Product, 20.0);
        latest
    }

    fn started(time_s: f32, latest: &LatestTemperatureReadings) -> RunSupervisor {
        let mut supervisor = RunSupervisor::new(SupervisorConfig::default());
        supervisor.handle_command(time_s, latest, RunCommand::Start);
        assert_eq!(supervisor.state(), RunState::Running);
        supervisor
    }

    #[test]
    fn new_supervisor_is_idle_and_requests_no_heat() {
        let supervisor = RunSupervisor::new(SupervisorConfig::default());

        assert_eq!(supervisor.state(), RunState::Idle);
        assert!(!supervisor.desired_heater_on());
    }

    #[test]
    fn cold_reading_does_not_activate_heating_while_idle() {
        let latest = fresh_readings(0.0, 18.0);
        let mut supervisor = RunSupervisor::new(SupervisorConfig::default());

        let outcome = supervisor.tick(0.0, &latest, RealRunUpdate::Probe(TemperatureProbe::BoxAir));

        assert_eq!(outcome.state, RunState::Idle);
        assert!(!outcome.desired_heater_on);
        assert_eq!(outcome.sample, None);
        assert!(!supervisor.desired_heater_on());
    }

    #[test]
    fn start_is_rejected_without_any_box_air_reading() {
        let latest = LatestTemperatureReadings::new();
        let mut supervisor = RunSupervisor::new(SupervisorConfig::default());

        let outcome = supervisor.handle_command(0.0, &latest, RunCommand::Start);

        assert_eq!(outcome.state, RunState::Idle);
        assert_eq!(outcome.state_change, None);
    }

    #[test]
    fn start_is_rejected_when_box_air_is_stale() {
        let latest = fresh_readings(0.0, 20.0);
        let mut supervisor = RunSupervisor::new(SupervisorConfig::default());
        let stale_time_s = RealRunConfig::default().max_box_air_age_s + 0.1;

        supervisor.handle_command(stale_time_s, &latest, RunCommand::Start);

        assert_eq!(supervisor.state(), RunState::Idle);
    }

    #[test]
    fn start_is_rejected_when_required_product_has_never_been_read() {
        let mut latest = LatestTemperatureReadings::new();
        latest.update_at(0.0, TemperatureProbe::BoxAir, 20.0);
        let mut supervisor = RunSupervisor::new(SupervisorConfig::default());

        supervisor.handle_command(0.0, &latest, RunCommand::Start);

        assert_eq!(supervisor.state(), RunState::Idle);
    }

    #[test]
    fn start_does_not_require_product_when_product_is_disabled() {
        let mut latest = LatestTemperatureReadings::new();
        latest.update_at(0.0, TemperatureProbe::BoxAir, 20.0);
        let mut supervisor =
            RunSupervisor::new(SupervisorConfig::new(RealRunConfig::default(), false));

        supervisor.handle_command(0.0, &latest, RunCommand::Start);

        assert_eq!(supervisor.state(), RunState::Running);
    }

    #[test]
    fn start_does_not_require_room_air() {
        let latest = fresh_readings(0.0, 20.0);
        let mut supervisor = RunSupervisor::new(SupervisorConfig::default());

        let outcome = supervisor.handle_command(0.0, &latest, RunCommand::Start);

        assert_eq!(outcome.state, RunState::Running);
        assert_eq!(
            outcome.state_change,
            Some(StateChange {
                state: RunState::Running,
                reason: "user_start",
            })
        );
        assert!(!outcome.desired_heater_on);
    }

    #[test]
    fn cold_valid_box_reading_requests_heating_while_running() {
        let latest = fresh_readings(0.0, 20.0);
        let mut supervisor = started(0.0, &latest);

        let outcome = supervisor.tick(0.0, &latest, RealRunUpdate::Probe(TemperatureProbe::BoxAir));

        assert_eq!(outcome.state, RunState::Running);
        assert!(outcome.desired_heater_on);
        assert_eq!(
            outcome.sample.map(|sample| sample.reason),
            Some("below_target".to_string())
        );
    }

    #[test]
    fn stop_disables_heating_immediately() {
        let latest = fresh_readings(0.0, 20.0);
        let mut supervisor = started(0.0, &latest);
        supervisor.tick(0.0, &latest, RealRunUpdate::Probe(TemperatureProbe::BoxAir));
        assert!(supervisor.desired_heater_on());

        let outcome = supervisor.handle_command(1.0, &latest, RunCommand::Stop);

        assert_eq!(outcome.state, RunState::Idle);
        assert!(!outcome.desired_heater_on);
        assert!(!supervisor.desired_heater_on());
    }

    #[test]
    fn stale_box_reading_enters_fault() {
        let latest = fresh_readings(0.0, 20.0);
        let mut supervisor = started(0.0, &latest);
        supervisor.tick(0.0, &latest, RealRunUpdate::Probe(TemperatureProbe::BoxAir));

        let stale_time_s = RealRunConfig::default().max_box_air_age_s + 0.1;
        let outcome = supervisor.tick(stale_time_s, &latest, RealRunUpdate::Tick);

        assert_eq!(outcome.state, RunState::Fault(FaultReason::BoxAirStale));
        assert!(!outcome.desired_heater_on);
    }

    #[test]
    fn stale_enabled_product_reading_enters_fault() {
        let mut latest = fresh_readings(0.0, 20.0);
        let mut supervisor = started(0.0, &latest);

        let stale_time_s = RealRunConfig::default().max_product_age_s + 0.1;
        latest.update_at(stale_time_s, TemperatureProbe::BoxAir, 20.0);
        let outcome = supervisor.tick(stale_time_s, &latest, RealRunUpdate::Tick);

        assert_eq!(outcome.state, RunState::Fault(FaultReason::ProductStale));
        assert!(!outcome.desired_heater_on);
    }

    #[test]
    fn box_air_hard_cutoff_enters_fault() {
        let latest = fresh_readings(0.0, 20.0);
        let mut supervisor = started(0.0, &latest);

        let hot = fresh_readings(1.0, 34.0);
        let outcome = supervisor.tick(1.0, &hot, RealRunUpdate::Tick);

        assert_eq!(
            outcome.state,
            RunState::Fault(FaultReason::BoxAirHardCutoff)
        );
        assert!(!outcome.desired_heater_on);
    }

    #[test]
    fn product_hard_cutoff_enters_fault() {
        let latest = fresh_readings(0.0, 20.0);
        let mut supervisor = started(0.0, &latest);

        let mut hot = LatestTemperatureReadings::new();
        hot.update_at(1.0, TemperatureProbe::BoxAir, 20.0);
        hot.update_at(1.0, TemperatureProbe::Product, 34.0);
        let outcome = supervisor.tick(1.0, &hot, RealRunUpdate::Tick);

        assert_eq!(
            outcome.state,
            RunState::Fault(FaultReason::ProductHardCutoff)
        );
        assert!(!outcome.desired_heater_on);
    }

    #[test]
    fn actuator_failure_enters_fault_and_latches_first_reason() {
        let latest = fresh_readings(0.0, 20.0);
        let mut supervisor = started(0.0, &latest);

        let outcome = supervisor.report_fault(FaultReason::ActuatorFailed);
        assert_eq!(outcome.state, RunState::Fault(FaultReason::ActuatorFailed));

        supervisor.report_fault(FaultReason::BoxAirStale);
        assert_eq!(
            supervisor.state(),
            RunState::Fault(FaultReason::ActuatorFailed)
        );
    }

    #[test]
    fn recovered_probe_does_not_clear_a_latched_fault() {
        let latest = fresh_readings(0.0, 20.0);
        let mut supervisor = started(0.0, &latest);

        let stale_time_s = RealRunConfig::default().max_box_air_age_s + 0.1;
        supervisor.tick(stale_time_s, &latest, RealRunUpdate::Tick);
        assert_eq!(
            supervisor.state(),
            RunState::Fault(FaultReason::BoxAirStale)
        );

        let recovered = fresh_readings(stale_time_s, 20.0);
        let outcome = supervisor.tick(
            stale_time_s,
            &recovered,
            RealRunUpdate::Probe(TemperatureProbe::BoxAir),
        );

        assert_eq!(outcome.state, RunState::Fault(FaultReason::BoxAirStale));
        assert!(!outcome.desired_heater_on);
    }

    #[test]
    fn fault_acknowledgement_enters_idle_not_running() {
        let latest = fresh_readings(0.0, 20.0);
        let mut supervisor = started(0.0, &latest);
        supervisor.report_fault(FaultReason::ActuatorFailed);

        let outcome = supervisor.handle_command(1.0, &latest, RunCommand::AcknowledgeFault);

        assert_eq!(outcome.state, RunState::Idle);
        assert_eq!(
            outcome.state_change,
            Some(StateChange {
                state: RunState::Idle,
                reason: "fault_acknowledged",
            })
        );
        assert!(!outcome.desired_heater_on);
    }

    #[test]
    fn fault_acknowledgement_is_rejected_while_conditions_are_unhealthy() {
        let latest = fresh_readings(0.0, 20.0);
        let mut supervisor = started(0.0, &latest);
        supervisor.report_fault(FaultReason::ActuatorFailed);

        let stale_time_s = RealRunConfig::default().max_box_air_age_s + 0.1;
        supervisor.handle_command(stale_time_s, &latest, RunCommand::AcknowledgeFault);

        assert_eq!(
            supervisor.state(),
            RunState::Fault(FaultReason::ActuatorFailed)
        );
    }

    #[test]
    fn start_is_ignored_while_in_fault() {
        let latest = fresh_readings(0.0, 20.0);
        let mut supervisor = started(0.0, &latest);
        supervisor.report_fault(FaultReason::ActuatorFailed);

        supervisor.handle_command(1.0, &latest, RunCommand::Start);

        assert_eq!(
            supervisor.state(),
            RunState::Fault(FaultReason::ActuatorFailed)
        );
    }

    #[test]
    fn acknowledgement_then_start_is_required_after_a_fault() {
        let latest = fresh_readings(0.0, 20.0);
        let mut supervisor = started(0.0, &latest);
        supervisor.report_fault(FaultReason::ActuatorFailed);

        supervisor.handle_command(1.0, &latest, RunCommand::AcknowledgeFault);
        assert_eq!(supervisor.state(), RunState::Idle);

        supervisor.handle_command(1.0, &latest, RunCommand::Start);
        assert_eq!(supervisor.state(), RunState::Running);
    }

    #[test]
    fn a_new_run_resets_the_controller() {
        let latest = fresh_readings(0.0, 20.0);
        let mut supervisor = started(0.0, &latest);
        supervisor.tick(0.0, &latest, RealRunUpdate::Probe(TemperatureProbe::BoxAir));
        assert!(supervisor.desired_heater_on());

        supervisor.handle_command(1.0, &latest, RunCommand::Stop);
        supervisor.handle_command(1.0, &latest, RunCommand::Start);

        assert!(!supervisor.desired_heater_on());
    }

    #[test]
    fn heating_is_never_requested_outside_running() {
        let commands = [
            RunCommand::Start,
            RunCommand::Stop,
            RunCommand::AcknowledgeFault,
        ];
        let updates = [
            RealRunUpdate::Tick,
            RealRunUpdate::Probe(TemperatureProbe::BoxAir),
            RealRunUpdate::Probe(TemperatureProbe::RoomAir),
            RealRunUpdate::Probe(TemperatureProbe::Product),
        ];
        let temperatures = [18.0_f32, 29.8, 30.5, 34.0, 40.0];

        for command in commands {
            for update in updates {
                for temp_c in temperatures {
                    for stale in [false, true] {
                        let mut supervisor = RunSupervisor::new(SupervisorConfig::default());
                        let latest = fresh_readings(0.0, temp_c);
                        let time_s = if stale {
                            RealRunConfig::default().max_box_air_age_s + 0.1
                        } else {
                            0.0
                        };

                        for step in 0..4 {
                            let outcome = supervisor.handle_command(time_s, &latest, command);
                            assert!(
                                outcome.state.is_running() || !outcome.desired_heater_on,
                                "command {command:?} step {step} requested heat in {:?}",
                                outcome.state
                            );

                            let outcome = supervisor.tick(time_s, &latest, update);
                            assert!(
                                outcome.state.is_running() || !outcome.desired_heater_on,
                                "update {update:?} step {step} requested heat in {:?}",
                                outcome.state
                            );
                            assert_eq!(
                                outcome.desired_heater_on,
                                supervisor.desired_heater_on(),
                                "outcome disagrees with supervisor"
                            );
                        }
                    }
                }
            }
        }
    }
}
