use std::collections::VecDeque;

use serde::Serialize;
use tempeh_protocol::status::parse_status_line;
use tempeh_protocol::{
    ParsedControlLine, parse_actuator_line, parse_control_line, parse_state_line,
    parse_temperature_line,
};

const EVENT_LIMIT: usize = 100;
const STALE_AFTER_S: f32 = 10.0;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DiagnosticEvent {
    pub time_s: f32,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TemperatureView {
    pub value_c: Option<f32>,
    pub age_s: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MonitorSnapshot {
    pub run_state: String,
    pub fault_reason: Option<String>,
    pub pause_reason: Option<String>,
    pub actuator_warning: Option<String>,
    pub interruption_age_s: Option<f32>,
    pub desired_heater_on: Option<bool>,
    pub confirmed_heater_on: Option<bool>,
    pub last_confirmed_heater_on: Option<bool>,
    pub confirmation_age_s: Option<f32>,
    pub lease_duration_s: Option<f32>,
    pub boot_id: Option<String>,
    pub segment: u64,
    pub room_air: TemperatureView,
    pub box_air: TemperatureView,
    pub product: TemperatureView,
    pub serial_age_s: Option<f32>,
    pub serial_stale: bool,
    pub status_age_s: Option<f32>,
    pub status_stale: bool,
    pub disconnected: bool,
    pub events: Vec<DiagnosticEvent>,
}

#[derive(Debug, Default)]
pub(crate) struct MonitorState {
    run_state: Option<String>,
    fault_reason: Option<String>,
    pause_reason: Option<String>,
    actuator_warning: Option<String>,
    interruption_started_at_s: Option<f32>,
    desired: Option<bool>,
    current_confirmed: Option<bool>,
    last_confirmed: Option<bool>,
    confirmed_at_s: Option<f32>,
    lease_duration_s: Option<f32>,
    boot_id: Option<String>,
    segment: u64,
    room_air: Option<(f32, f32)>,
    box_air: Option<(f32, f32)>,
    product: Option<(f32, f32)>,
    last_serial_s: Option<f32>,
    last_status_s: Option<f32>,
    disconnected: bool,
    events: VecDeque<DiagnosticEvent>,
}

impl MonitorState {
    pub(crate) fn segment(&self) -> u64 {
        self.segment
    }

    pub(crate) fn event(&mut self, now_s: f32, kind: &str, message: impl Into<String>) {
        if self.events.len() == EVENT_LIMIT {
            self.events.pop_front();
        }
        self.events.push_back(DiagnosticEvent {
            time_s: now_s,
            kind: kind.into(),
            message: message.into(),
        });
    }

    pub(crate) fn disconnect(&mut self, now_s: f32) {
        self.disconnected = true;
        self.event(now_s, "connection", "ESP32 serial connection closed");
    }

    pub(crate) fn ingest(
        &mut self,
        line: &str,
        now_s: f32,
    ) -> Result<Option<ParsedControlLine>, String> {
        self.last_serial_s = Some(now_s);
        if let Some(status) = parse_status_line(line)? {
            let state_changed = self.run_state.as_deref() != Some(status.run_state.as_str());
            if self.boot_id.as_deref() != Some(&status.boot_id) {
                if self.boot_id.is_some() {
                    self.segment += 1;
                    self.event(now_s, "reboot", "ESP32 started a new boot");
                }
                self.boot_id = Some(status.boot_id.clone());
                self.fault_reason = None;
                self.pause_reason = None;
                self.actuator_warning = None;
                self.interruption_started_at_s = None;
            }
            self.last_status_s = Some(now_s);
            self.run_state = Some(status.run_state.clone());
            self.fault_reason = status.fault_reason.clone();
            self.pause_reason = status.pause_reason.clone();
            self.actuator_warning = status.actuator_warning.clone();
            self.interruption_started_at_s = status
                .interruption_started_s
                .map(|at| now_s - (status.uptime_s - at));
            self.desired = Some(status.desired_heater_on);
            self.current_confirmed = status.confirmed_heater_on;
            self.last_confirmed = status.last_confirmed_heater_on;
            self.lease_duration_s = Some(status.lease_duration_s);
            self.confirmed_at_s = status
                .last_successful_command_s
                .map(|at| now_s - (status.uptime_s - at));
            self.room_air = observed(status.room_air.temp_c, status.room_air.age_s, now_s);
            self.box_air = observed(status.box_air.temp_c, status.box_air.age_s, now_s);
            self.product = observed(status.product.temp_c, status.product.age_s, now_s);
            if state_changed {
                self.event(
                    now_s,
                    "state",
                    format!(
                        "{}: {}",
                        status.run_state,
                        status.fault_reason.as_deref().unwrap_or("status")
                    ),
                );
            }
            return Ok(None);
        }
        if let Some(state) = parse_state_line(line).map_err(|e| format!("{e:?}"))? {
            let changed = self.run_state.as_deref() != Some(&state.state);
            self.run_state = Some(state.state.clone());
            if state.state == "fault" {
                if self.fault_reason.is_none() {
                    self.fault_reason = Some(state.reason.clone());
                }
                self.desired = Some(false);
            } else if state.state == "paused" {
                self.fault_reason = None;
                self.pause_reason = Some(state.reason.clone());
                self.actuator_warning = None;
                self.desired = Some(false);
            } else {
                self.fault_reason = None;
                self.pause_reason = None;
                self.actuator_warning = None;
                self.interruption_started_at_s = None;
            }
            if changed || state.state == "fault" {
                self.event(now_s, "state", format!("{}: {}", state.state, state.reason));
            }
            return Ok(None);
        }
        if let Some(actuator) = parse_actuator_line(line).map_err(|e| format!("{e:?}"))? {
            self.desired = Some(actuator.desired_heater_on);
            self.current_confirmed = actuator.confirmed_heater_on;
            if let Some(value) = actuator.confirmed_heater_on {
                self.last_confirmed = Some(value);
                self.confirmed_at_s = Some(now_s);
            }
            self.event(now_s, "actuator", actuator.reason.clone());
            if actuator.reason == "renewal_reply_missing" {
                self.actuator_warning = Some("on_reply_missing".into());
                self.interruption_started_at_s.get_or_insert(now_s);
            } else if actuator.reason == "lease_renewed" || actuator.reason == "resume_on_confirmed"
            {
                self.actuator_warning = None;
                self.interruption_started_at_s = None;
            }
            return Ok(None);
        }
        if let Some(temp) = parse_temperature_line(line).map_err(|e| format!("{e:?}"))? {
            match temp.probe {
                tempeh_model::TemperatureProbe::BoxAir => self.box_air = Some((temp.temp_c, now_s)),
                tempeh_model::TemperatureProbe::RoomAir => {
                    self.room_air = Some((temp.temp_c, now_s))
                }
                tempeh_model::TemperatureProbe::Product => {
                    self.product = Some((temp.temp_c, now_s))
                }
            }
            return Ok(None);
        }
        if let Some(control) = parse_control_line(line).map_err(|e| format!("{e:?}"))? {
            if self.run_state.as_deref() != Some("fault") {
                self.desired = Some(control.heater_on);
            }
            return Ok(Some(control));
        }
        if line.contains("WARN") || line.contains("ERROR") || line.contains("failed") {
            self.event(now_s, "firmware", line.trim());
        }
        Ok(None)
    }

    pub(crate) fn snapshot(&self, now_s: f32) -> MonitorSnapshot {
        let confirmation_age_s = self.confirmed_at_s.map(|at| (now_s - at).max(0.0));
        let confirmed_heater_on = if self
            .lease_duration_s
            .is_some_and(|duration| confirmation_age_s.is_some_and(|age| age < duration))
        {
            self.current_confirmed
        } else {
            None
        };
        let serial_age_s = self.last_serial_s.map(|at| (now_s - at).max(0.0));
        let status_age_s = self.last_status_s.map(|at| (now_s - at).max(0.0));
        MonitorSnapshot {
            run_state: self.run_state.clone().unwrap_or_else(|| "unknown".into()),
            fault_reason: self.fault_reason.clone(),
            pause_reason: self.pause_reason.clone(),
            actuator_warning: self.actuator_warning.clone(),
            interruption_age_s: self
                .interruption_started_at_s
                .map(|at| (now_s - at).max(0.0)),
            desired_heater_on: self.desired,
            confirmed_heater_on,
            last_confirmed_heater_on: self.last_confirmed,
            confirmation_age_s,
            lease_duration_s: self.lease_duration_s,
            boot_id: self.boot_id.clone(),
            segment: self.segment,
            room_air: view(self.room_air, now_s),
            box_air: view(self.box_air, now_s),
            product: view(self.product, now_s),
            serial_age_s,
            serial_stale: self.disconnected || serial_age_s.is_none_or(|age| age >= STALE_AFTER_S),
            status_age_s,
            status_stale: self
                .last_status_s
                .is_some_and(|at| now_s - at >= STALE_AFTER_S),
            disconnected: self.disconnected,
            events: self.events.iter().cloned().collect(),
        }
    }
}

fn observed(value: Option<f32>, age_s: Option<f32>, now_s: f32) -> Option<(f32, f32)> {
    value.zip(age_s).map(|(value, age)| (value, now_s - age))
}

fn view(observed: Option<(f32, f32)>, now_s: f32) -> TemperatureView {
    TemperatureView {
        value_c: observed.map(|(value, _)| value),
        age_s: observed.map(|(_, at)| (now_s - at).max(0.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempeh_protocol::status::{ProbeStatus, STATUS_VERSION, StatusRecord, format_status_line};

    fn status(boot_id: &str, uptime_s: f32, state: &str) -> String {
        format_status_line(&StatusRecord {
            version: STATUS_VERSION,
            boot_id: boot_id.into(),
            uptime_s,
            run_state: state.into(),
            fault_reason: (state == "fault").then(|| "actuator_failed".into()),
            pause_reason: None,
            actuator_warning: None,
            interruption_started_s: None,
            desired_heater_on: state == "running",
            actuator_ready: state != "fault",
            confirmed_heater_on: (state == "running").then_some(true),
            last_confirmed_heater_on: Some(true),
            last_successful_command_s: Some(uptime_s - 1.0),
            last_successful_renewal_s: Some(uptime_s - 1.0),
            lease_duration_s: 20.0,
            room_air: ProbeStatus {
                temp_c: None,
                age_s: None,
            },
            box_air: ProbeStatus {
                temp_c: Some(21.0),
                age_s: Some(2.0),
            },
            product: ProbeStatus {
                temp_c: Some(22.0),
                age_s: Some(2.0),
            },
        })
    }

    #[test]
    fn renewal_failure_is_visible_after_control_samples_stop() {
        let mut monitor = MonitorState::default();
        monitor
            .ingest(&status("one", 10.0, "running"), 10.0)
            .unwrap();
        monitor
            .ingest("control,10,,21,22,1,below_target", 10.0)
            .unwrap();
        monitor
            .ingest("actuator,15,1,,lease_renewal_failed", 15.0)
            .unwrap();
        monitor
            .ingest("state,15,fault,actuator_failed", 15.0)
            .unwrap();
        let view = monitor.snapshot(16.0);
        assert_eq!(view.run_state, "fault");
        assert_eq!(view.fault_reason.as_deref(), Some("actuator_failed"));
        assert_eq!(view.desired_heater_on, Some(false));
        assert_eq!(view.confirmed_heater_on, None);
        assert_eq!(view.last_confirmed_heater_on, Some(true));
        assert_eq!(view.box_air.age_s, Some(8.0));
    }

    #[test]
    fn late_attachment_and_local_expiry_work_without_new_samples() {
        let mut monitor = MonitorState::default();
        monitor.ingest(&status("one", 10.0, "fault"), 50.0).unwrap();
        assert_eq!(
            monitor.snapshot(50.0).fault_reason.as_deref(),
            Some("actuator_failed")
        );
        assert_eq!(monitor.snapshot(69.0).confirmed_heater_on, None);
        let view = monitor.snapshot(60.0);
        assert!(view.serial_stale);
        assert!(view.status_stale);
    }

    #[test]
    fn state_recovery_and_reboot_do_not_reuse_old_faults() {
        let mut monitor = MonitorState::default();
        monitor.ingest("state,4,fault,product_stale", 4.0).unwrap();
        monitor
            .ingest("actuator,5,0,0,actuator_recovered", 5.0)
            .unwrap();
        assert_eq!(
            monitor.snapshot(5.0).fault_reason.as_deref(),
            Some("product_stale")
        );
        assert_eq!(monitor.snapshot(5.0).confirmed_heater_on, None);
        monitor
            .ingest(&status("one", 10.0, "running"), 10.0)
            .unwrap();
        monitor.ingest(&status("two", 1.0, "idle"), 11.0).unwrap();
        let view = monitor.snapshot(11.0);
        assert_eq!(view.segment, 1);
        assert_eq!(view.fault_reason, None);
    }

    #[test]
    fn malformed_record_does_not_destroy_previous_state() {
        let mut monitor = MonitorState::default();
        monitor.ingest("state,4,fault,box_air_stale", 4.0).unwrap();
        assert!(monitor.ingest("status,{bad json", 5.0).is_err());
        assert_eq!(
            monitor.snapshot(5.0).fault_reason.as_deref(),
            Some("box_air_stale")
        );
        monitor.disconnect(6.0);
        assert!(monitor.snapshot(6.0).disconnected);
    }

    #[test]
    fn paused_status_shows_interruption_age_and_auto_resume_clears_it() {
        let mut paused: serde_json::Value =
            serde_json::from_str(&status("one", 20.0, "paused")[7..]).unwrap();
        paused["pause_reason"] = "actuator_unreachable".into();
        paused["interruption_started_s"] = 12.0.into();
        let mut monitor = MonitorState::default();
        monitor.ingest(&format!("status,{paused}"), 100.0).unwrap();

        let view = monitor.snapshot(103.0);
        assert_eq!(view.run_state, "paused");
        assert_eq!(view.pause_reason.as_deref(), Some("actuator_unreachable"));
        assert_eq!(view.interruption_age_s, Some(11.0));
        assert_eq!(view.desired_heater_on, Some(false));

        monitor
            .ingest("state,23,running,actuator_recovered_auto_resume", 104.0)
            .unwrap();
        let view = monitor.snapshot(104.0);
        assert_eq!(view.pause_reason, None);
        assert_eq!(view.interruption_age_s, None);
    }
}
