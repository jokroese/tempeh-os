use std::time::Duration;

use anyhow::Result;
use esp_idf_hal::gpio::Gpio48;
use esp_idf_hal::rmt::{
    CHANNEL0, FixedLengthSignal, PinState, Pulse, TxRmtDriver, config::TransmitConfig,
};
use tempeh_runtime::run_supervisor::RunState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Booting,
    Idle,
    Running,
    Retrying,
    Paused,
    Fault,
}

impl From<RunState> for Status {
    fn from(state: RunState) -> Self {
        match state {
            RunState::Idle => Self::Idle,
            RunState::Running => Self::Running,
            RunState::Paused => Self::Paused,
            RunState::Fault(_) => Self::Fault,
        }
    }
}

pub struct StatusLed {
    tx: TxRmtDriver<'static>,
    current: Option<Status>,
}

impl StatusLed {
    pub fn new(channel: CHANNEL0, pin: Gpio48) -> Result<Self> {
        let config = TransmitConfig::new().clock_divider(1);
        Ok(Self {
            tx: TxRmtDriver::new(channel, pin, &config)?,
            current: None,
        })
    }

    pub fn show(&mut self, status: Status) -> Result<()> {
        if self.current == Some(status) {
            return Ok(());
        }

        let rgb = match status {
            Status::Booting => Rgb::new(12, 5, 0),
            Status::Idle => Rgb::new(0, 0, 12),
            Status::Running => Rgb::new(0, 12, 0),
            Status::Retrying => Rgb::new(12, 9, 0),
            Status::Paused => Rgb::new(8, 0, 10),
            Status::Fault => Rgb::new(12, 0, 0),
        };
        self.write(rgb)?;
        self.current = Some(status);
        Ok(())
    }

    fn write(&mut self, rgb: Rgb) -> Result<()> {
        let colour: u32 = rgb.into();
        let ticks_hz = self.tx.counter_clock()?;
        let t0h = Pulse::new_with_duration(ticks_hz, PinState::High, &Duration::from_nanos(350))?;
        let t0l = Pulse::new_with_duration(ticks_hz, PinState::Low, &Duration::from_nanos(800))?;
        let t1h = Pulse::new_with_duration(ticks_hz, PinState::High, &Duration::from_nanos(700))?;
        let t1l = Pulse::new_with_duration(ticks_hz, PinState::Low, &Duration::from_nanos(600))?;
        let mut signal = FixedLengthSignal::<24>::new();

        for bit_index in (0..24).rev() {
            let bit_is_set = colour & (1_u32 << bit_index) != 0;
            let pulses = if bit_is_set { (t1h, t1l) } else { (t0h, t0l) };
            signal.set(23 - bit_index as usize, &pulses)?;
        }

        self.tx.start_blocking(&signal)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct Rgb {
    red: u8,
    green: u8,
    blue: u8,
}

impl Rgb {
    const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }
}

impl From<Rgb> for u32 {
    fn from(rgb: Rgb) -> Self {
        ((rgb.green as u32) << 16) | ((rgb.red as u32) << 8) | rgb.blue as u32
    }
}
