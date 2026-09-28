#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeaseConfig {
    lease_duration_s: f32,
    renewal_interval_s: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseConfigError {
    NonFinite,
    NonPositiveRenewalInterval,
    LeaseTooShort,
}

impl LeaseConfig {
    pub fn new(lease_duration_s: f32, renewal_interval_s: f32) -> Result<Self, LeaseConfigError> {
        let config = Self {
            lease_duration_s,
            renewal_interval_s,
        };

        if !lease_duration_s.is_finite() || !renewal_interval_s.is_finite() {
            return Err(LeaseConfigError::NonFinite);
        }
        if renewal_interval_s <= 0.0 {
            return Err(LeaseConfigError::NonPositiveRenewalInterval);
        }
        if lease_duration_s < 3.0 * renewal_interval_s {
            return Err(LeaseConfigError::LeaseTooShort);
        }

        Ok(config)
    }

    pub fn is_safe(&self) -> bool {
        self.lease_duration_s.is_finite()
            && self.renewal_interval_s.is_finite()
            && self.renewal_interval_s > 0.0
            && self.lease_duration_s >= 3.0 * self.renewal_interval_s
    }

    pub fn lease_duration_s(&self) -> f32 {
        self.lease_duration_s
    }

    pub fn renewal_interval_s(&self) -> f32 {
        self.renewal_interval_s
    }
}

impl Default for LeaseConfig {
    fn default() -> Self {
        Self {
            lease_duration_s: 20.0,
            renewal_interval_s: 5.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseAction {
    None,
    SendOn,
    SendOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmedHeaterState {
    Unknown,
    Off,
    On,
}

/// Leaves at least 5 seconds of the 20-second plug lease for safe-off recovery.
pub const ON_RETRY_DEADLINE_S: f32 = 15.0;

impl ConfirmedHeaterState {
    pub fn as_option(self) -> Option<bool> {
        match self {
            Self::Unknown => None,
            Self::Off => Some(false),
            Self::On => Some(true),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeaterLease {
    config: LeaseConfig,
    desired_on: bool,
    confirmed_state: ConfirmedHeaterState,
    last_successful_state: Option<bool>,
    due_at_s: f32,
    last_successful_command_s: Option<f32>,
    last_successful_renewal_s: Option<f32>,
    ambiguous_on_until_s: Option<f32>,
}

impl HeaterLease {
    pub fn new(config: LeaseConfig) -> Self {
        Self {
            config,
            desired_on: false,
            confirmed_state: ConfirmedHeaterState::Unknown,
            last_successful_state: None,
            due_at_s: 0.0,
            last_successful_command_s: None,
            last_successful_renewal_s: None,
            ambiguous_on_until_s: None,
        }
    }

    pub fn set_desired(&mut self, on: bool, now_s: f32) {
        if on == self.desired_on {
            return;
        }
        self.desired_on = on;
        self.due_at_s = now_s;
    }

    pub fn poll(&mut self, now_s: f32) -> LeaseAction {
        if now_s < self.due_at_s {
            return LeaseAction::None;
        }

        if self.desired_on {
            self.due_at_s = now_s + self.config.renewal_interval_s;
            LeaseAction::SendOn
        } else if self.confirmed_state != ConfirmedHeaterState::Off
            || self.ambiguous_on_until_s.is_some_and(|until| now_s < until)
        {
            self.due_at_s = now_s
                + if self.ambiguous_on_until_s.is_some_and(|until| now_s < until) {
                    2.0
                } else {
                    self.config.renewal_interval_s
                };
            LeaseAction::SendOff
        } else {
            LeaseAction::None
        }
    }

    pub fn record_success(&mut self, now_s: f32, on: bool) {
        self.confirmed_state = if on {
            ConfirmedHeaterState::On
        } else {
            ConfirmedHeaterState::Off
        };
        self.last_successful_command_s = Some(now_s);
        self.last_successful_state = Some(on);
        if on {
            self.last_successful_renewal_s = Some(now_s);
            self.due_at_s = now_s + self.config.renewal_interval_s;
        } else if self.ambiguous_on_until_s.is_some_and(|until| now_s < until) {
            self.due_at_s = now_s + 2.0;
        }
    }

    pub fn record_failure(&mut self, now_s: f32) {
        self.confirmed_state = ConfirmedHeaterState::Unknown;
        self.due_at_s = now_s
            + if self.ambiguous_on_until_s.is_some_and(|until| now_s < until) {
                2.0
            } else {
                self.config.renewal_interval_s
            };
    }

    /// An unanswered ON may still execute after its HTTP request times out.
    pub fn record_ambiguous_on_failure(&mut self, now_s: f32) {
        self.record_failure(now_s);
        self.due_at_s = now_s + 1.0;
        self.ambiguous_on_until_s = Some(now_s + self.config.lease_duration_s);
    }

    pub fn ambiguous_on_pending(&self, now_s: f32) -> bool {
        self.ambiguous_on_until_s.is_some_and(|until| now_s < until)
    }

    pub fn desired_heater_on(&self) -> bool {
        self.desired_on
    }

    pub fn confirmed_heater_state(&self) -> ConfirmedHeaterState {
        self.confirmed_state
    }

    pub fn confirmed_heater_on(&self) -> Option<bool> {
        self.confirmed_state.as_option()
    }

    /// The last command reply is evidence only for one lease duration.
    /// This does not affect the state used to schedule commands.
    pub fn current_confirmation(&self, now_s: f32) -> Option<bool> {
        let confirmed_at_s = self.last_successful_command_s?;
        if !now_s.is_finite()
            || now_s < confirmed_at_s
            || now_s - confirmed_at_s >= self.config.lease_duration_s
        {
            return None;
        }
        self.confirmed_heater_on()
    }

    pub fn last_successful_state(&self) -> Option<bool> {
        self.last_successful_state
    }

    pub fn lease_duration_s(&self) -> f32 {
        self.config.lease_duration_s
    }

    pub fn last_successful_command_s(&self) -> Option<f32> {
        self.last_successful_command_s
    }

    pub fn last_successful_renewal_s(&self) -> Option<f32> {
        self.last_successful_renewal_s
    }

    pub fn lease_expires_at_s(&self) -> Option<f32> {
        self.last_successful_renewal_s
            .map(|renewed_at_s| renewed_at_s + self.config.lease_duration_s)
    }

    pub fn lease_expired(&self, now_s: f32) -> bool {
        self.lease_expires_at_s()
            .is_none_or(|expires_at_s| now_s >= expires_at_s)
    }

    pub fn on_retry_window_expired(&self, now_s: f32) -> bool {
        self.last_successful_state != Some(true)
            || self
                .last_successful_renewal_s
                .is_none_or(|at| now_s >= at + ON_RETRY_DEADLINE_S)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lease() -> HeaterLease {
        HeaterLease::new(LeaseConfig::default())
    }

    #[test]
    fn confirmations_expire_for_both_power_states_without_changing_scheduling() {
        let mut lease = lease();
        assert_eq!(lease.current_confirmation(0.0), None);
        lease.set_desired(true, 0.0);
        assert_eq!(lease.poll(0.0), LeaseAction::SendOn);
        lease.record_success(0.0, true);
        assert_eq!(lease.current_confirmation(19.999), Some(true));
        assert_eq!(lease.current_confirmation(20.0), None);
        assert_eq!(lease.poll(20.0), LeaseAction::SendOn);

        lease.set_desired(false, 21.0);
        assert_eq!(lease.poll(21.0), LeaseAction::SendOff);
        lease.record_success(21.0, false);
        assert_eq!(lease.current_confirmation(40.999), Some(false));
        assert_eq!(lease.current_confirmation(41.0), None);
        assert_eq!(lease.poll(41.0), LeaseAction::None);
    }

    #[test]
    fn failed_command_preserves_history_but_invalidates_confirmation() {
        let mut lease = lease();
        lease.record_success(3.0, true);
        lease.record_failure(4.0);
        assert_eq!(lease.current_confirmation(4.0), None);
        assert_eq!(lease.last_successful_state(), Some(true));
        assert_eq!(lease.last_successful_command_s(), Some(3.0));
        lease.record_success(5.0, true);
        assert_eq!(lease.current_confirmation(5.0), Some(true));
        assert_eq!(lease.current_confirmation(f32::NAN), None);
    }

    #[test]
    fn default_lease_is_at_least_three_renewal_intervals() {
        let config = LeaseConfig::default();

        assert_eq!(config.lease_duration_s(), 20.0);
        assert_eq!(config.renewal_interval_s(), 5.0);
        assert!(config.is_safe());
    }

    #[test]
    fn rejects_a_lease_shorter_than_three_renewal_intervals() {
        assert_eq!(
            LeaseConfig::new(10.0, 5.0),
            Err(LeaseConfigError::LeaseTooShort)
        );
    }

    #[test]
    fn rejects_non_finite_lease_values() {
        assert_eq!(
            LeaseConfig::new(f32::INFINITY, 5.0),
            Err(LeaseConfigError::NonFinite)
        );
        assert_eq!(
            LeaseConfig::new(20.0, f32::NAN),
            Err(LeaseConfigError::NonFinite)
        );
    }

    #[test]
    fn rejects_a_non_positive_renewal_interval() {
        assert_eq!(
            LeaseConfig::new(20.0, 0.0),
            Err(LeaseConfigError::NonPositiveRenewalInterval)
        );
        assert_eq!(
            LeaseConfig::new(20.0, -1.0),
            Err(LeaseConfigError::NonPositiveRenewalInterval)
        );
    }

    #[test]
    fn constructs_a_safe_custom_lease() {
        let config = LeaseConfig::new(30.0, 10.0).expect("safe lease config");

        assert_eq!(config.lease_duration_s(), 30.0);
        assert_eq!(config.renewal_interval_s(), 10.0);
        assert!(config.is_safe());
    }

    #[test]
    fn a_new_lease_sends_off_and_confirms_nothing() {
        let mut lease = lease();

        assert_eq!(lease.poll(0.0), LeaseAction::SendOff);
        assert_eq!(
            lease.confirmed_heater_state(),
            ConfirmedHeaterState::Unknown
        );
        assert_eq!(lease.confirmed_heater_on(), None);
        assert_eq!(lease.last_successful_renewal_s(), None);
        assert_eq!(lease.lease_expires_at_s(), None);
        assert!(lease.lease_expired(0.0));
    }

    #[test]
    fn requesting_heat_sends_on_immediately() {
        let mut lease = lease();
        lease.poll(0.0);

        lease.set_desired(true, 1.0);

        assert_eq!(lease.poll(1.0), LeaseAction::SendOn);
    }

    #[test]
    fn renews_at_the_renewal_interval_while_heating() {
        let mut lease = lease();
        lease.set_desired(true, 0.0);

        assert_eq!(lease.poll(0.0), LeaseAction::SendOn);
        lease.record_success(0.0, true);

        assert_eq!(lease.poll(4.9), LeaseAction::None);
        assert_eq!(lease.poll(5.0), LeaseAction::SendOn);
        lease.record_success(5.0, true);

        assert_eq!(lease.last_successful_renewal_s(), Some(5.0));
        assert_eq!(lease.lease_expires_at_s(), Some(25.0));
    }

    #[test]
    fn never_sends_on_while_heat_is_not_desired() {
        let mut lease = lease();
        lease.set_desired(true, 0.0);
        lease.poll(0.0);
        lease.record_success(0.0, true);
        lease.set_desired(false, 1.0);

        let mut now_s = 1.0;
        while now_s < 120.0 {
            assert_ne!(lease.poll(now_s), LeaseAction::SendOn);
            now_s += 0.05;
        }
    }

    #[test]
    fn stopping_sends_off_once_and_then_remains_quiet() {
        let mut lease = lease();
        lease.set_desired(true, 0.0);
        lease.poll(0.0);
        lease.record_success(0.0, true);

        lease.set_desired(false, 2.0);
        assert_eq!(lease.poll(2.0), LeaseAction::SendOff);
        lease.record_success(2.0, false);

        assert_eq!(lease.poll(6.9), LeaseAction::None);
        assert_eq!(lease.poll(7.0), LeaseAction::None);
        assert_eq!(lease.poll(120.0), LeaseAction::None);
    }

    #[test]
    fn a_failed_renewal_does_not_advance_the_lease() {
        let mut lease = lease();
        lease.set_desired(true, 0.0);
        lease.poll(0.0);
        lease.record_success(0.0, true);
        assert_eq!(lease.lease_expires_at_s(), Some(20.0));

        lease.poll(5.0);
        lease.record_failure(5.0);

        assert_eq!(
            lease.confirmed_heater_state(),
            ConfirmedHeaterState::Unknown
        );
        assert_eq!(lease.confirmed_heater_on(), None);
        assert_eq!(lease.last_successful_renewal_s(), Some(0.0));
        assert_eq!(lease.lease_expires_at_s(), Some(20.0));
        assert!(!lease.lease_expired(19.9));
        assert!(lease.lease_expired(20.0));
    }

    #[test]
    fn a_failed_renewal_retries_after_the_renewal_interval() {
        let mut lease = lease();
        lease.set_desired(true, 0.0);
        lease.poll(0.0);
        lease.record_failure(0.0);

        assert_eq!(lease.poll(4.9), LeaseAction::None);
        assert_eq!(lease.poll(5.0), LeaseAction::SendOn);
    }

    #[test]
    fn unanswered_on_retries_soon_without_extending_the_confirmed_lease() {
        let mut lease = lease();
        lease.set_desired(true, 0.0);
        assert_eq!(lease.poll(0.0), LeaseAction::SendOn);
        lease.record_success(0.0, true);
        assert_eq!(lease.poll(5.0), LeaseAction::SendOn);
        lease.record_ambiguous_on_failure(7.0);

        assert_eq!(lease.current_confirmation(7.0), None);
        assert_eq!(lease.last_successful_renewal_s(), Some(0.0));
        assert!(!lease.on_retry_window_expired(14.9));
        assert!(lease.on_retry_window_expired(15.0));
        assert_eq!(lease.poll(7.9), LeaseAction::None);
        assert_eq!(lease.poll(8.0), LeaseAction::SendOn);
        lease.record_success(8.1, true);
        assert_eq!(lease.last_successful_renewal_s(), Some(8.1));
        assert_eq!(lease.poll(13.0), LeaseAction::None);
    }

    #[test]
    fn first_on_failure_has_no_confirmed_retry_window() {
        let mut lease = lease();
        lease.set_desired(true, 0.0);
        assert_eq!(lease.poll(0.0), LeaseAction::SendOn);
        lease.record_ambiguous_on_failure(2.0);
        assert!(lease.on_retry_window_expired(2.0));
        assert_eq!(lease.current_confirmation(2.0), None);
    }

    #[test]
    fn a_confirmed_off_ends_the_previous_on_retry_window() {
        let mut lease = lease();
        lease.set_desired(true, 0.0);
        lease.record_success(0.0, true);
        lease.set_desired(false, 1.0);
        lease.record_success(1.0, false);
        lease.set_desired(true, 2.0);
        lease.record_ambiguous_on_failure(4.0);
        assert!(lease.on_retry_window_expired(4.0));
    }

    #[test]
    fn unanswered_on_followed_by_stop_repeats_off_for_one_lease() {
        let mut lease = lease();
        lease.set_desired(true, 0.0);
        assert_eq!(lease.poll(0.0), LeaseAction::SendOn);
        lease.record_ambiguous_on_failure(2.0);
        lease.set_desired(false, 3.0);
        assert_eq!(lease.poll(3.0), LeaseAction::SendOff);
        lease.record_success(3.0, false);
        assert_eq!(lease.poll(4.9), LeaseAction::None);
        assert_eq!(lease.poll(5.0), LeaseAction::SendOff);
        lease.record_success(5.0, false);
        assert_eq!(lease.poll(21.9), LeaseAction::SendOff);
        lease.record_success(21.9, false);
        assert_eq!(lease.poll(22.0), LeaseAction::None);
    }

    #[test]
    fn a_successful_off_does_not_renew_the_lease() {
        let mut lease = lease();
        lease.set_desired(true, 0.0);
        lease.poll(0.0);
        lease.record_success(0.0, true);

        lease.set_desired(false, 1.0);
        lease.poll(1.0);
        lease.record_success(1.0, false);

        assert_eq!(lease.last_successful_command_s(), Some(1.0));
        assert_eq!(lease.last_successful_renewal_s(), Some(0.0));
        assert_eq!(lease.confirmed_heater_on(), Some(false));
    }

    #[test]
    fn a_failed_off_command_makes_the_confirmed_state_unknown() {
        let mut lease = lease();
        lease.set_desired(true, 0.0);
        lease.poll(0.0);
        lease.record_success(0.0, true);
        assert_eq!(lease.confirmed_heater_on(), Some(true));

        lease.set_desired(false, 1.0);
        lease.poll(1.0);
        lease.record_failure(1.0);

        assert_eq!(
            lease.confirmed_heater_state(),
            ConfirmedHeaterState::Unknown
        );
        assert_eq!(lease.confirmed_heater_on(), None);
    }

    #[test]
    fn renewal_is_never_later_than_the_renewal_interval_while_heating() {
        let mut lease = lease();
        lease.set_desired(true, 0.0);

        let mut now_s = 0.0;
        let mut last_renewal_s = 0.0;
        while now_s < 300.0 {
            if lease.poll(now_s) == LeaseAction::SendOn {
                lease.record_success(now_s, true);
                last_renewal_s = now_s;
            }
            assert!(
                now_s - last_renewal_s <= LeaseConfig::default().renewal_interval_s,
                "renewal gap exceeded at {now_s}"
            );
            now_s += 0.05;
        }
    }
}
