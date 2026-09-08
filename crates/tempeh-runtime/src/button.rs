use crate::run_supervisor::{RunCommand, RunState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonConfig {
    pub debounce_ms: u32,
    pub hold_ms: u32,
}

impl Default for ButtonConfig {
    fn default() -> Self {
        Self {
            debounce_ms: 50,
            hold_ms: 2_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ButtonReader {
    config: ButtonConfig,
    awaiting_release: bool,
    raw_pressed: bool,
    raw_since_ms: u32,
    stable_pressed: bool,
    stable_since_ms: u32,
    fired: bool,
}

impl ButtonReader {
    pub fn new(config: ButtonConfig) -> Self {
        Self {
            config,
            awaiting_release: true,
            raw_pressed: true,
            raw_since_ms: 0,
            stable_pressed: true,
            stable_since_ms: 0,
            fired: false,
        }
    }

    pub fn poll(&mut self, pressed: bool, now_ms: u32, state: RunState) -> Option<RunCommand> {
        if pressed != self.raw_pressed {
            self.raw_pressed = pressed;
            self.raw_since_ms = now_ms;
        }

        if pressed != self.stable_pressed
            && now_ms.wrapping_sub(self.raw_since_ms) >= self.config.debounce_ms
        {
            self.stable_pressed = pressed;
            self.stable_since_ms = now_ms;
            if !pressed {
                self.awaiting_release = false;
                self.fired = false;
            }
        }

        if self.awaiting_release || !self.stable_pressed || self.fired {
            return None;
        }

        let held_ms = now_ms.wrapping_sub(self.stable_since_ms);
        let command = match state {
            RunState::Running => Some(RunCommand::Stop),
            RunState::Idle if held_ms >= self.config.hold_ms => Some(RunCommand::Start),
            RunState::Fault(_) if held_ms >= self.config.hold_ms => {
                Some(RunCommand::AcknowledgeFault)
            }
            _ => None,
        };

        if command.is_some() {
            self.fired = true;
        }

        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_supervisor::FaultReason;

    fn hold(
        reader: &mut ButtonReader,
        state: RunState,
        from_ms: u32,
        to_ms: u32,
        pressed: bool,
    ) -> Option<RunCommand> {
        let mut command = None;
        let mut now_ms = from_ms;
        while now_ms <= to_ms {
            if let Some(next) = reader.poll(pressed, now_ms, state) {
                command = command.or(Some(next));
            }
            now_ms += 10;
        }
        command
    }

    #[test]
    fn ignores_a_press_held_through_boot_until_it_is_released() {
        let mut reader = ButtonReader::new(ButtonConfig::default());

        assert_eq!(hold(&mut reader, RunState::Idle, 0, 5_000, true), None);

        hold(&mut reader, RunState::Idle, 5_010, 5_200, false);
        assert_eq!(
            hold(&mut reader, RunState::Idle, 5_210, 8_000, true),
            Some(RunCommand::Start)
        );
    }

    fn released_reader() -> ButtonReader {
        let mut reader = ButtonReader::new(ButtonConfig::default());
        hold(&mut reader, RunState::Idle, 0, 200, false);
        reader
    }

    #[test]
    fn a_two_second_hold_starts_a_run_from_idle() {
        let mut reader = released_reader();

        assert_eq!(
            hold(&mut reader, RunState::Idle, 210, 2_400, true),
            Some(RunCommand::Start)
        );
    }

    #[test]
    fn a_short_press_does_not_start_a_run() {
        let mut reader = released_reader();

        assert_eq!(hold(&mut reader, RunState::Idle, 210, 1_500, true), None);
    }

    #[test]
    fn a_debounced_press_stops_a_run_without_a_hold() {
        let mut reader = released_reader();

        assert_eq!(
            hold(&mut reader, RunState::Running, 210, 300, true),
            Some(RunCommand::Stop)
        );
    }

    #[test]
    fn a_bounce_shorter_than_the_debounce_interval_is_ignored() {
        let mut reader = released_reader();

        assert_eq!(reader.poll(true, 210, RunState::Running), None);
        assert_eq!(reader.poll(false, 230, RunState::Running), None);
        assert_eq!(reader.poll(false, 300, RunState::Running), None);
    }

    #[test]
    fn a_two_second_hold_acknowledges_a_fault() {
        let mut reader = released_reader();
        let state = RunState::Fault(FaultReason::BoxAirStale);

        assert_eq!(
            hold(&mut reader, state, 210, 2_400, true),
            Some(RunCommand::AcknowledgeFault)
        );
    }

    #[test]
    fn a_short_press_does_not_acknowledge_a_fault() {
        let mut reader = released_reader();
        let state = RunState::Fault(FaultReason::BoxAirStale);

        assert_eq!(hold(&mut reader, state, 210, 1_500, true), None);
    }

    #[test]
    fn one_press_produces_at_most_one_command() {
        let mut reader = released_reader();
        let mut commands = 0;

        let mut now_ms = 210;
        while now_ms <= 10_000 {
            if reader.poll(true, now_ms, RunState::Idle).is_some() {
                commands += 1;
            }
            now_ms += 10;
        }

        assert_eq!(commands, 1);
    }

    #[test]
    fn a_hold_that_starts_a_run_does_not_also_stop_it() {
        let mut reader = released_reader();
        let mut state = RunState::Idle;

        let mut now_ms = 210;
        let mut commands = Vec::new();
        while now_ms <= 6_000 {
            if let Some(command) = reader.poll(true, now_ms, state) {
                commands.push(command);
                if command == RunCommand::Start {
                    state = RunState::Running;
                }
            }
            now_ms += 10;
        }

        assert_eq!(commands, vec![RunCommand::Start]);
    }

    #[test]
    fn a_second_press_after_release_produces_another_command() {
        let mut reader = released_reader();

        assert_eq!(
            hold(&mut reader, RunState::Running, 210, 400, true),
            Some(RunCommand::Stop)
        );
        hold(&mut reader, RunState::Idle, 410, 600, false);
        assert_eq!(
            hold(&mut reader, RunState::Idle, 610, 2_800, true),
            Some(RunCommand::Start)
        );
    }
}
