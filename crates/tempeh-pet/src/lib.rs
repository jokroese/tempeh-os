use tempeh_model::{ControllerConfig, EnvironmentState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TempehMood {
    Sleepy,
    WarmingUp,
    Thriving,
    Spicy,
    Panicking,
    Finished,
}

impl TempehMood {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sleepy => "Sleepy",
            Self::WarmingUp => "Warming up",
            Self::Thriving => "Thriving",
            Self::Spicy => "Getting spicy",
            Self::Panicking => "Panicking",
            Self::Finished => "Finished",
        }
    }

    pub fn emoji(self) -> &'static str {
        match self {
            Self::Sleepy => "🌙",
            Self::WarmingUp => "🔥",
            Self::Thriving => "🍄",
            Self::Spicy => "🌶️",
            Self::Panicking => "🚨",
            Self::Finished => "✨",
        }
    }

    pub fn css_class(self) -> &'static str {
        match self {
            Self::Sleepy => "sleepy",
            Self::WarmingUp => "warming",
            Self::Thriving => "thriving",
            Self::Spicy => "spicy",
            Self::Panicking => "panicking",
            Self::Finished => "finished",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PetState {
    pub mood: TempehMood,
    pub mycelium_confidence: f32,
    pub safety_margin_c: f32,
    pub estimated_ready_in_s: Option<f32>,
}

impl PetState {
    pub fn headline(&self, name: &str) -> String {
        format!(
            "{} {name} is {}",
            self.mood.emoji(),
            self.mood.label().to_lowercase()
        )
    }

    pub fn message(&self) -> &'static str {
        match self.mood {
            TempehMood::Sleepy => "I am quiet. Keep me warm and give the spores time to wake.",
            TempehMood::WarmingUp => {
                "I am coming up to temperature. Warmth is arriving, but the culture is not fully cruising yet."
            }
            TempehMood::Thriving => "I am making my own warmth now. Do not smother me.",
            TempehMood::Spicy => {
                "I am running hot. Watch the core temperature and consider more airflow."
            }
            TempehMood::Panicking => {
                "Too hot. Heater locked off; intervene before the batch suffers."
            }
            TempehMood::Finished => "The cake is ready. Cool, cook, or refrigerate me.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PetReport {
    pub state: EnvironmentState,
    pub pet: PetState,
}

pub fn analyse_pet(
    state: EnvironmentState,
    config: ControllerConfig,
    estimated_ready_in_s: Option<f32>,
) -> PetState {
    let safety_margin_c = config.hard_tempeh_cutoff_c.min(config.hard_box_cutoff_c)
        - state.tempeh_core_temp_c.max(state.box_air_temp_c);

    let mycelium_confidence = mycelium_confidence(state, config);
    let mood = mood(state, config, mycelium_confidence);

    PetState {
        mood,
        mycelium_confidence,
        safety_margin_c,
        estimated_ready_in_s,
    }
}

pub fn mood(
    state: EnvironmentState,
    config: ControllerConfig,
    mycelium_confidence: f32,
) -> TempehMood {
    if state.box_air_temp_c >= config.hard_box_cutoff_c
        || state.tempeh_core_temp_c >= config.hard_tempeh_cutoff_c
    {
        TempehMood::Panicking
    } else if state.fermentation_progress >= 0.95 {
        TempehMood::Finished
    } else if state.tempeh_core_temp_c >= 35.0 || state.box_air_temp_c >= 33.0 {
        TempehMood::Spicy
    } else if mycelium_confidence >= 0.35 && state.metabolic_heat_rate_c_per_s >= 0.00004 {
        TempehMood::Thriving
    } else if state.box_air_temp_c < config.target_box_air_temp_c - 1.0 {
        TempehMood::WarmingUp
    } else {
        TempehMood::Sleepy
    }
}

pub fn mycelium_confidence(state: EnvironmentState, config: ControllerConfig) -> f32 {
    let temp_score = temp_score(state.tempeh_core_temp_c);
    let safety_score = safety_score(state, config);
    let heat_score = (state.metabolic_heat_rate_c_per_s / 0.00012).clamp(0.0, 1.0);
    let progress_score = state.fermentation_progress.clamp(0.0, 1.0);

    (0.45 * progress_score + 0.25 * temp_score + 0.2 * heat_score + 0.1 * safety_score)
        .clamp(0.0, 1.0)
}

fn temp_score(temp_c: f32) -> f32 {
    if temp_c < 24.0 {
        0.0
    } else if temp_c < 28.0 {
        (temp_c - 24.0) / 4.0
    } else if temp_c <= 32.0 {
        1.0
    } else if temp_c <= 36.0 {
        1.0 - ((temp_c - 32.0) / 4.0) * 0.7
    } else {
        0.0
    }
}

fn safety_score(state: EnvironmentState, config: ControllerConfig) -> f32 {
    let hot_margin = config.hard_tempeh_cutoff_c.min(config.hard_box_cutoff_c)
        - state.tempeh_core_temp_c.max(state.box_air_temp_c);

    (hot_margin / 5.0).clamp(0.0, 1.0)
}

pub fn report_for_samples(
    samples: &[EnvironmentState],
    config: ControllerConfig,
) -> Option<PetReport> {
    let state = samples.last().copied()?;
    let estimated_ready_in_s = estimate_ready_in_s(samples);
    let pet = analyse_pet(state, config, estimated_ready_in_s);
    Some(PetReport { state, pet })
}

pub fn estimate_ready_in_s(samples: &[EnvironmentState]) -> Option<f32> {
    let current = samples.last()?;

    if current.fermentation_progress >= 0.95 {
        return Some(0.0);
    }

    let previous = samples
        .iter()
        .rev()
        .find(|sample| sample.time_s < current.time_s)?;

    let progress_delta = current.fermentation_progress - previous.fermentation_progress;
    let time_delta_s = current.time_s - previous.time_s;

    if progress_delta <= 0.0 || time_delta_s <= 0.0 {
        return None;
    }

    let progress_per_s = progress_delta / time_delta_s;
    let remaining = 0.95 - current.fermentation_progress;

    Some((remaining / progress_per_s).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(
        box_air_temp_c: f32,
        tempeh_core_temp_c: f32,
        fermentation_progress: f32,
        metabolic_heat_rate_c_per_s: f32,
    ) -> EnvironmentState {
        EnvironmentState {
            time_s: 0.0,
            room_air_temp_c: 20.0,
            box_air_temp_c,
            tempeh_core_temp_c,
            fermentation_progress,
            metabolic_heat_rate_c_per_s,
            heater_on: false,
        }
    }

    #[test]
    fn pet_panics_at_hard_cutoff() {
        let config = ControllerConfig::default();
        let state = state(30.0, config.hard_tempeh_cutoff_c + 0.1, 0.4, 0.0001);
        let pet = analyse_pet(state, config, None);
        assert_eq!(pet.mood, TempehMood::Panicking);
    }

    #[test]
    fn pet_finishes_near_complete_progress() {
        let config = ControllerConfig::default();
        let state = state(30.0, 31.0, 0.96, 0.00002);
        let pet = analyse_pet(state, config, None);
        assert_eq!(pet.mood, TempehMood::Finished);
    }

    #[test]
    fn pet_thrives_when_progressing_and_warm() {
        let config = ControllerConfig::default();
        let state = state(30.0, 31.0, 0.55, 0.00011);
        let pet = analyse_pet(state, config, None);
        assert_eq!(pet.mood, TempehMood::Thriving);
        assert!(pet.mycelium_confidence > 0.35);
    }

    #[test]
    fn ready_estimate_uses_recent_progress_rate() {
        let samples = vec![
            EnvironmentState {
                time_s: 0.0,
                room_air_temp_c: 20.0,
                box_air_temp_c: 30.0,
                tempeh_core_temp_c: 30.0,
                fermentation_progress: 0.50,
                metabolic_heat_rate_c_per_s: 0.0001,
                heater_on: true,
            },
            EnvironmentState {
                time_s: 100.0,
                room_air_temp_c: 20.0,
                box_air_temp_c: 30.0,
                tempeh_core_temp_c: 30.0,
                fermentation_progress: 0.60,
                metabolic_heat_rate_c_per_s: 0.0001,
                heater_on: true,
            },
        ];

        let estimate = estimate_ready_in_s(&samples).expect("estimate");
        assert!((estimate - 350.0).abs() < 0.01);
    }
}
