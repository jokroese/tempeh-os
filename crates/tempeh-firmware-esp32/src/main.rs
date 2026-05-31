use anyhow::{Context, Result, bail};
use embedded_svc::http::client::Client;
use esp_idf_hal::delay::{Ets, FreeRtos};
use esp_idf_hal::modem::Modem;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::http::client::{Configuration as HttpConfiguration, EspHttpConnection};
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use esp_idf_sys::{
    esp, esp_get_free_heap_size, esp_get_minimum_free_heap_size, esp_timer_get_time, gpio_config,
    gpio_config_t, gpio_get_level, gpio_int_type_t_GPIO_INTR_DISABLE,
    gpio_mode_t_GPIO_MODE_INPUT_OUTPUT_OD, gpio_num_t, gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
    gpio_pullup_t_GPIO_PULLUP_ENABLE, gpio_set_level, uxTaskGetStackHighWaterMark,
    xTaskGetCurrentTaskHandle,
};
use log::{info, warn};
use tempeh_model::TemperatureProbe;
use tempeh_protocol::probe_name;
use tempeh_runtime::{LatestTemperatureReadings, RealRunConfig, RealRunController, RealRunSample};

const BOX_AIR_GPIO: i32 = 5;
const ROOM_AIR_GPIO: i32 = 6;
const PRODUCT_GPIO: i32 = 4;

const DS18B20_SKIP_ROM: u8 = 0xCC;
const DS18B20_CONVERT_T: u8 = 0x44;
const DS18B20_READ_SCRATCHPAD: u8 = 0xBE;
const WIFI_SSID: Option<&str> = option_env!("TEMPEH_WIFI_SSID");
const WIFI_PASSWORD: Option<&str> = option_env!("TEMPEH_WIFI_PASSWORD");
const TASMOTA_BASE_URL: Option<&str> = option_env!("TEMPEH_TASMOTA_BASE_URL");
const PROBE_BOX_AIR: Option<&str> = option_env!("TEMPEH_PROBE_BOX_AIR");
const PROBE_ROOM_AIR: Option<&str> = option_env!("TEMPEH_PROBE_ROOM_AIR");
const PROBE_PRODUCT: Option<&str> = option_env!("TEMPEH_PROBE_PRODUCT");

#[derive(Debug, Clone, Copy)]
struct ProbeConfig {
    box_air: bool,
    room_air: bool,
    product: bool,
}

fn parse_probe_bool(value: Option<&str>, default: bool) -> bool {
    match value {
        Some("true") => true,
        Some("false") => false,
        _ => default,
    }
}

impl ProbeConfig {
    fn from_build_config() -> Result<Self> {
        let probes = Self {
            box_air: parse_probe_bool(PROBE_BOX_AIR, true),
            room_air: parse_probe_bool(PROBE_ROOM_AIR, false),
            product: parse_probe_bool(PROBE_PRODUCT, true),
        };

        if !probes.box_air {
            bail!(
                "probe box_air must be enabled for control; set [probes].box_air = true in firmware.local.toml"
            );
        }

        Ok(probes)
    }
}

fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;
    let probes = ProbeConfig::from_build_config()?;

    if probes.box_air {
        let _box_air_pin = peripherals.pins.gpio5;
    }
    if probes.room_air {
        let _room_air_pin = peripherals.pins.gpio6;
    }
    if probes.product {
        let _product_pin = peripherals.pins.gpio4;
    }
    let modem = peripherals.modem;

    let _wifi = connect_wifi(modem)?;
    let mut heater_output = TasmotaHeaterOutput::from_build_config()?;

    // Start from a known safe state before reading probes or running policy.
    heater_output.set_heater(false, "boot_safe_off")?;

    let mut box_air = probes
        .box_air
        .then(|| Ds18b20::new(BOX_AIR_GPIO))
        .transpose()?;
    let mut room_air = probes
        .room_air
        .then(|| Ds18b20::new(ROOM_AIR_GPIO))
        .transpose()?;
    let mut product = probes
        .product
        .then(|| Ds18b20::new(PRODUCT_GPIO))
        .transpose()?;

    let mut latest = LatestTemperatureReadings::new();
    let mut controller = RealRunController::new(RealRunConfig::default());
    let start_us = now_us();

    info!("Tempeh OS ESP32 temperature bridge");
    if probes.box_air {
        info!("box_air DATA -> GPIO{BOX_AIR_GPIO}");
    }
    if probes.room_air {
        info!("room_air DATA -> GPIO{ROOM_AIR_GPIO}");
    }
    if probes.product {
        info!("product DATA -> GPIO{PRODUCT_GPIO}");
    }
    info!(
        "enabled probes: box_air={}, room_air={}, product={}",
        probes.box_air, probes.room_air, probes.product
    );
    info!("running shared real-run policy on device");
    info!("heater output: Tasmota HTTP; runtime decisions actuate the plug");
    info!("actuator mode: send command only when desired heater state changes");
    info!(
        "control output: control,time_s,room_air_temp_c,box_air_temp_c,product_temp_c,heater_on,reason"
    );

    let mut last_diagnostics_s = 0.0_f32;
    let mut last_safety_tick_s = 0.0_f32;

    loop {
        let time_s = elapsed_s(start_us);
        maybe_log_runtime_diagnostics(time_s, &mut last_diagnostics_s, last_safety_tick_s);
        let mut emitted_control_this_sweep = false;

        if let Some(probe) = box_air.as_mut() {
            emitted_control_this_sweep |= read_update_and_print(
                TemperatureProbe::BoxAir,
                probe,
                &mut latest,
                &mut controller,
                &mut heater_output,
                start_us,
            )?;
        }
        if let Some(probe) = room_air.as_mut() {
            emitted_control_this_sweep |= read_update_and_print(
                TemperatureProbe::RoomAir,
                probe,
                &mut latest,
                &mut controller,
                &mut heater_output,
                start_us,
            )?;
        }
        if let Some(probe) = product.as_mut() {
            emitted_control_this_sweep |= read_update_and_print(
                TemperatureProbe::Product,
                probe,
                &mut latest,
                &mut controller,
                &mut heater_output,
                start_us,
            )?;
        }

        if emitted_control_this_sweep {
            last_safety_tick_s = time_s;
        } else if run_safety_tick(&latest, &mut controller, &mut heater_output, start_us)? {
            last_safety_tick_s = time_s;
        }

        FreeRtos::delay_ms(2_000);
    }
}

fn connect_wifi(modem: Modem) -> Result<BlockingWifi<EspWifi<'static>>> {
    let ssid = WIFI_SSID.unwrap_or_default();
    let password = WIFI_PASSWORD.unwrap_or_default();

    if ssid.is_empty() {
        bail!(
            "TEMPEH_WIFI_SSID is not set at build time. Copy firmware.local.example.toml to firmware.local.toml and set [wifi] ssid/password."
        );
    }

    let sysloop = EspSystemEventLoop::take().context("failed to take ESP system event loop")?;
    let nvs = EspDefaultNvsPartition::take().context("failed to take default NVS partition")?;

    let wifi = EspWifi::new(modem, sysloop.clone(), Some(nvs)).context("failed to create Wi-Fi")?;
    let mut wifi = BlockingWifi::wrap(wifi, sysloop).context("failed to wrap blocking Wi-Fi")?;

    let configuration = Configuration::Client(ClientConfiguration {
        ssid: ssid
            .try_into()
            .map_err(|_| anyhow::anyhow!("TEMPEH_WIFI_SSID is too long"))?,
        password: password
            .try_into()
            .map_err(|_| anyhow::anyhow!("TEMPEH_WIFI_PASSWORD is too long"))?,
        ..Default::default()
    });

    info!("connecting Wi-Fi to SSID {ssid:?}");
    wifi.set_configuration(&configuration)
        .context("failed to configure Wi-Fi")?;
    wifi.start().context("failed to start Wi-Fi")?;
    wifi.connect().context("failed to connect Wi-Fi")?;
    wifi.wait_netif_up()
        .context("Wi-Fi netif did not come up")?;

    let ip_info = wifi
        .wifi()
        .sta_netif()
        .get_ip_info()
        .context("failed to read Wi-Fi IP info")?;

    info!(
        "Wi-Fi connected: ip={}, subnet={}, gateway={}",
        ip_info.ip, ip_info.subnet.mask, ip_info.subnet.gateway
    );

    Ok(wifi)
}

#[derive(Debug, Clone)]
struct TasmotaHeaterOutput {
    base_url: String,
    heater_on: bool,
}

impl TasmotaHeaterOutput {
    fn from_build_config() -> Result<Self> {
        let base_url = TASMOTA_BASE_URL.unwrap_or_default();

        if base_url.is_empty() {
            bail!(
                "TEMPEH_TASMOTA_BASE_URL is not set. Add [tasmota].base_url to firmware.local.toml"
            );
        }

        Ok(Self::new(base_url))
    }

    fn heater_on(&self) -> bool {
        self.heater_on
    }

    fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: normalise_tasmota_base_url(base_url.into()),
            heater_on: false,
        }
    }

    fn apply_decision(&mut self, heater_on: bool, reason: &'static str) -> Result<()> {
        if heater_on == self.heater_on() {
            return Ok(());
        }
        self.set_heater_fail_safe(heater_on, reason)
    }

    fn set_heater(&mut self, on: bool, reason: &'static str) -> Result<()> {
        let url = self.command_url(on);
        let command_label = if on { "on" } else { "off" };

        info!("sending Tasmota heater {command_label} command: reason={reason}");
        info!("Tasmota command URL: {url}");

        let connection = EspHttpConnection::new(&HttpConfiguration::default())
            .context("failed to create ESP HTTP connection")?;
        let mut client = Client::wrap(connection);
        let request = client
            .get(&url)
            .with_context(|| format!("failed to create Tasmota request: {url}"))?;
        let response = request
            .submit()
            .with_context(|| format!("failed to send Tasmota request: {url}"))?;

        let status = response.status();
        if !(200..300).contains(&status) {
            bail!("Tasmota heater command failed with HTTP status {status}");
        }

        self.heater_on = on;
        info!(
            "Tasmota heater command accepted: heater_on={}",
            self.heater_on
        );
        Ok(())
    }

    fn set_heater_fail_safe(&mut self, on: bool, reason: &'static str) -> Result<()> {
        match self.set_heater(on, reason) {
            Ok(()) => Ok(()),
            Err(error) if on => {
                warn!(
                    "failed to turn heater on for reason={reason}; attempting fail-safe off: {error:#}"
                );

                if let Err(off_error) = self.set_heater(false, "actuator_on_failed_safe_off") {
                    warn!(
                        "fail-safe off command also failed after actuator-on error: {off_error:#}"
                    );
                }

                Err(error).context("failed to turn heater on; fail-safe off attempted")
            }
            Err(error) => {
                warn!(
                    "failed to turn heater off for reason={reason}; actuator state is unknown: {error:#}"
                );
                self.heater_on = false;
                Err(error).context("failed to turn heater off; actuator state is unknown")
            }
        }
    }

    fn command_url(&self, on: bool) -> String {
        let command = if on { "Power%20On" } else { "Power%20Off" };
        format!("{}/cm?cmnd={command}", self.base_url)
    }
}

fn normalise_tasmota_base_url(mut base_url: String) -> String {
    base_url = base_url.trim().to_string();

    if let Some((before_query, _query)) = base_url.split_once('?') {
        base_url = before_query.to_string();
    }

    base_url = base_url.trim_end_matches('/').to_string();

    if base_url.starts_with("http://") || base_url.starts_with("https://") {
        base_url
    } else {
        format!("http://{base_url}")
    }
}

fn read_update_and_print(
    probe_kind: TemperatureProbe,
    probe: &mut Ds18b20,
    latest: &mut LatestTemperatureReadings,
    controller: &mut RealRunController,
    heater_output: &mut TasmotaHeaterOutput,
    start_us: i64,
) -> Result<bool> {
    match probe.read_temperature_c() {
        Ok(temp_c) => {
            let time_s = elapsed_s(start_us);

            println!("temp,{},{temp_c:.3}", probe_name(probe_kind));

            latest.update_at(time_s, probe_kind, temp_c);

            return emit_control_sample(time_s, probe_kind, latest, controller, heater_output);
        }
        Err(error) => {
            warn!("{probe_kind:?} read failed: {error:#}");
        }
    }

    Ok(false)
}

fn run_safety_tick(
    latest: &LatestTemperatureReadings,
    controller: &mut RealRunController,
    heater_output: &mut TasmotaHeaterOutput,
    start_us: i64,
) -> Result<bool> {
    let time_s = elapsed_s(start_us);

    if let Some(sample) = controller.tick_sample(time_s, latest) {
        apply_and_print_control_sample(sample, heater_output)?;
        return Ok(true);
    }

    Ok(false)
}

fn emit_control_sample(
    time_s: f32,
    updated_probe: TemperatureProbe,
    latest: &LatestTemperatureReadings,
    controller: &mut RealRunController,
    heater_output: &mut TasmotaHeaterOutput,
) -> Result<bool> {
    if let Some(sample) = controller.update_sample(time_s, latest, updated_probe) {
        apply_and_print_control_sample(sample, heater_output)?;
        return Ok(true);
    }

    Ok(false)
}

fn apply_and_print_control_sample(
    sample: RealRunSample,
    heater_output: &mut TasmotaHeaterOutput,
) -> Result<()> {
    heater_output.apply_decision(sample.heater_on, sample.reason)?;
    print_control_sample(sample);
    Ok(())
}

fn print_control_sample(sample: RealRunSample) {
    let heater_on = if sample.heater_on { 1 } else { 0 };
    match (sample.room_air_temp_c, sample.product_temp_c) {
        (Some(room_air), Some(product)) => println!(
            "control,{:.0},{:.3},{:.3},{:.3},{},{}",
            sample.time_s, room_air, sample.box_air_temp_c, product, heater_on, sample.reason
        ),
        (Some(room_air), None) => println!(
            "control,{:.0},{:.3},{:.3},{},{},{}",
            sample.time_s, room_air, sample.box_air_temp_c, "", heater_on, sample.reason
        ),
        (None, Some(product)) => println!(
            "control,{:.0},{},{:.3},{:.3},{},{}",
            sample.time_s, "", sample.box_air_temp_c, product, heater_on, sample.reason
        ),
        (None, None) => println!(
            "control,{:.0},{},{:.3},{},{},{}",
            sample.time_s, "", sample.box_air_temp_c, "", heater_on, sample.reason
        ),
    }
}

fn maybe_log_runtime_diagnostics(
    time_s: f32,
    last_diagnostics_s: &mut f32,
    last_safety_tick_s: f32,
) {
    const DIAGNOSTICS_INTERVAL_S: f32 = 60.0;
    if time_s - *last_diagnostics_s < DIAGNOSTICS_INTERVAL_S {
        return;
    }
    *last_diagnostics_s = time_s;

    unsafe {
        let task = xTaskGetCurrentTaskHandle();
        let stack_high_water_words = uxTaskGetStackHighWaterMark(task);
        let free_heap = esp_get_free_heap_size();
        let min_free_heap = esp_get_minimum_free_heap_size();
        info!(
            "runtime diagnostics: main_stack_high_water={stack_high_water_words} words, free_heap={free_heap} bytes, min_free_heap={min_free_heap} bytes, last_safety_tick_s={last_safety_tick_s:.0}"
        );
    }
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

    #[test]
    fn normalise_tasmota_base_url_trims_and_adds_scheme() {
        assert_eq!(
            normalise_tasmota_base_url("192.168.8.193".into()),
            "http://192.168.8.193"
        );
        assert_eq!(
            normalise_tasmota_base_url("http://192.168.8.193/".into()),
            "http://192.168.8.193"
        );
    }
}
