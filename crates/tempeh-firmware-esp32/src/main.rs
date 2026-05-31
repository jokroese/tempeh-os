use anyhow::{Context, Result, bail};
use esp_idf_hal::delay::{Ets, FreeRtos};
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use esp_idf_sys::{
    esp, esp_timer_get_time, gpio_config, gpio_config_t, gpio_get_level,
    gpio_int_type_t_GPIO_INTR_DISABLE, gpio_mode_t_GPIO_MODE_INPUT_OUTPUT_OD, gpio_num_t,
    gpio_pulldown_t_GPIO_PULLDOWN_DISABLE, gpio_pullup_t_GPIO_PULLUP_ENABLE, gpio_set_level,
};
use log::{info, warn};
use tempeh_model::TemperatureProbe;
use tempeh_protocol::format_temperature_line;
use tempeh_runtime::{LatestTemperatureReadings, RealRunConfig, RealRunController, RealRunSample};

const BOX_AIR_GPIO: i32 = 5;
const ROOM_AIR_GPIO: i32 = 6;
const PRODUCT_GPIO: i32 = 4;

const DS18B20_SKIP_ROM: u8 = 0xCC;
const DS18B20_CONVERT_T: u8 = 0x44;
const DS18B20_READ_SCRATCHPAD: u8 = 0xBE;

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;
    let _box_air_pin = peripherals.pins.gpio5;
    let _room_air_pin = peripherals.pins.gpio6;
    let _product_pin = peripherals.pins.gpio4;

    let mut box_air = Ds18b20::new(BOX_AIR_GPIO)?;
    let mut room_air = Ds18b20::new(ROOM_AIR_GPIO)?;
    let mut product = Ds18b20::new(PRODUCT_GPIO)?;

    let mut latest = LatestTemperatureReadings::new();
    let mut controller = RealRunController::new(RealRunConfig::default());
    let start_us = now_us();

    info!("Tempeh OS ESP32 temperature bridge");
    info!("box_air DATA -> GPIO{BOX_AIR_GPIO}");
    info!("room_air DATA -> GPIO{ROOM_AIR_GPIO}");
    info!("product DATA -> GPIO{PRODUCT_GPIO}");
    info!("reading three DS18B20 probes on separate 1-Wire buses");
    info!("running shared real-run policy on device");
    info!(
        "control output: control,time_s,room_air_temp_c,box_air_temp_c,product_temp_c,heater_on,reason"
    );

    loop {
        read_update_and_print(
            TemperatureProbe::BoxAir,
            &mut box_air,
            &mut latest,
            &mut controller,
            start_us,
        );
        read_update_and_print(
            TemperatureProbe::RoomAir,
            &mut room_air,
            &mut latest,
            &mut controller,
            start_us,
        );
        read_update_and_print(
            TemperatureProbe::Product,
            &mut product,
            &mut latest,
            &mut controller,
            start_us,
        );

        FreeRtos::delay_ms(2_000);
    }
}

fn read_update_and_print(
    probe_kind: TemperatureProbe,
    probe: &mut Ds18b20,
    latest: &mut LatestTemperatureReadings,
    controller: &mut RealRunController,
    start_us: i64,
) {
    match probe.read_temperature_c() {
        Ok(temp_c) => {
            let time_s = elapsed_s(start_us);

            println!("{}", format_temperature_line(probe_kind, temp_c));

            latest.update_at(time_s, probe_kind, temp_c);

            if let Some(sample) = controller.update_sample(time_s, latest, probe_kind) {
                println!("{}", control_sample_line(sample));
            }
        }
        Err(error) => {
            warn!("{probe_kind:?} read failed: {error:#}");
        }
    }
}

fn control_sample_line(sample: RealRunSample) -> String {
    format!("control,{}", sample.csv_row())
}

fn now_us() -> i64 {
    unsafe { esp_timer_get_time() }
}

fn elapsed_s(start_us: i64) -> f32 {
    let elapsed_us = now_us().saturating_sub(start_us);
    elapsed_us as f32 / 1_000_000.0
}

struct Ds18b20 {
    bus: OneWireBus,
}

impl Ds18b20 {
    fn new(gpio: i32) -> Result<Self> {
        Ok(Self {
            bus: OneWireBus::new(gpio)?,
        })
    }

    fn read_temperature_c(&mut self) -> Result<f32> {
        self.bus
            .reset()
            .context("DS18B20 did not respond to reset")?;
        self.bus.write_byte(DS18B20_SKIP_ROM)?;
        self.bus.write_byte(DS18B20_CONVERT_T)?;

        // 12-bit DS18B20 conversion time is up to 750 ms.
        FreeRtos::delay_ms(750);

        self.bus
            .reset()
            .context("DS18B20 did not respond before scratchpad read")?;
        self.bus.write_byte(DS18B20_SKIP_ROM)?;
        self.bus.write_byte(DS18B20_READ_SCRATCHPAD)?;

        let mut scratchpad = [0_u8; 9];
        for byte in &mut scratchpad {
            *byte = self.bus.read_byte()?;
        }

        let expected_crc = scratchpad[8];
        let actual_crc = crc8(&scratchpad[..8]);
        if actual_crc != expected_crc {
            bail!("scratchpad CRC mismatch: expected {expected_crc:#04x}, got {actual_crc:#04x}");
        }

        let raw = i16::from_le_bytes([scratchpad[0], scratchpad[1]]);
        Ok(raw as f32 / 16.0)
    }
}

struct OneWireBus {
    gpio: gpio_num_t,
}

impl OneWireBus {
    fn new(gpio: i32) -> Result<Self> {
        let bus = Self { gpio };
        bus.configure()?;
        bus.release()?;
        Ok(bus)
    }

    fn configure(&self) -> Result<()> {
        let config = gpio_config_t {
            pin_bit_mask: 1_u64 << self.gpio,
            mode: gpio_mode_t_GPIO_MODE_INPUT_OUTPUT_OD,
            pull_up_en: gpio_pullup_t_GPIO_PULLUP_ENABLE,
            pull_down_en: gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
            intr_type: gpio_int_type_t_GPIO_INTR_DISABLE,
        };

        esp!(unsafe { gpio_config(&config) }).context("failed to configure 1-Wire GPIO")
    }

    fn reset(&mut self) -> Result<()> {
        self.drive_low()?;
        Ets::delay_us(480_u32);
        self.release()?;
        Ets::delay_us(70_u32);

        let present = self.read_level()? == 0;

        Ets::delay_us(410_u32);

        if present {
            Ok(())
        } else {
            bail!("no presence pulse on GPIO{}", self.gpio)
        }
    }

    fn write_byte(&mut self, byte: u8) -> Result<()> {
        for bit in 0..8 {
            self.write_bit(((byte >> bit) & 1) != 0)?;
        }
        Ok(())
    }

    fn read_byte(&mut self) -> Result<u8> {
        let mut byte = 0_u8;

        for bit in 0..8 {
            if self.read_bit()? {
                byte |= 1 << bit;
            }
        }

        Ok(byte)
    }

    fn write_bit(&mut self, bit: bool) -> Result<()> {
        if bit {
            self.drive_low()?;
            Ets::delay_us(6_u32);
            self.release()?;
            Ets::delay_us(64_u32);
        } else {
            self.drive_low()?;
            Ets::delay_us(60_u32);
            self.release()?;
            Ets::delay_us(10_u32);
        }

        Ok(())
    }

    fn read_bit(&mut self) -> Result<bool> {
        self.drive_low()?;
        Ets::delay_us(6_u32);
        self.release()?;
        Ets::delay_us(9_u32);

        let bit = self.read_level()? != 0;

        Ets::delay_us(55_u32);

        Ok(bit)
    }

    fn drive_low(&mut self) -> Result<()> {
        esp!(unsafe { gpio_set_level(self.gpio, 0) }).context("failed to drive 1-Wire bus low")
    }

    fn release(&self) -> Result<()> {
        esp!(unsafe { gpio_set_level(self.gpio, 1) }).context("failed to release 1-Wire bus")
    }

    fn read_level(&self) -> Result<i32> {
        Ok(unsafe { gpio_get_level(self.gpio) })
    }
}

fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0_u8;

    for byte in bytes {
        let mut value = *byte;
        for _ in 0..8 {
            let mix = (crc ^ value) & 0x01;
            crc >>= 1;
            if mix != 0 {
                crc ^= 0x8C;
            }
            value >>= 1;
        }
    }

    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc8_matches_ds18b20_scratchpad_example() {
        let scratchpad = [0x50, 0x05, 0x4B, 0x46, 0x7F, 0xFF, 0x0C, 0x10, 0x1C];
        assert_eq!(crc8(&scratchpad[..8]), scratchpad[8]);
    }
}
