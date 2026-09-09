mod mqtt;
mod status_led;
mod tasmota;

use anyhow::{Context, Result, anyhow, bail};
use esp_idf_hal::delay::{Ets, FreeRtos};
use esp_idf_hal::modem::Modem;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use esp_idf_sys::{
    esp, esp_get_free_heap_size, esp_get_minimum_free_heap_size, esp_timer_get_time, gpio_config,
    gpio_config_t, gpio_get_level, gpio_int_type_t_GPIO_INTR_DISABLE, gpio_mode_t_GPIO_MODE_INPUT,
    gpio_mode_t_GPIO_MODE_INPUT_OUTPUT_OD, gpio_num_t, gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
    gpio_pullup_t_GPIO_PULLUP_ENABLE, gpio_set_level, uxTaskGetStackHighWaterMark,
    xTaskGetCurrentTaskHandle,
};
use log::{info, warn};
use tempeh_model::TemperatureProbe;
use tempeh_protocol::tasmota::pulse_time_value_for_seconds;
use tempeh_protocol::{format_actuator_line, format_control_line, format_state_line, probe_name};
use tempeh_runtime::button::{ButtonConfig, ButtonReader};
use tempeh_runtime::heater_lease::{HeaterLease, LeaseAction, LeaseConfig};
use tempeh_runtime::run_supervisor::{
    FaultReason, RunState, RunSupervisor, SupervisorConfig, SupervisorOutcome,
};
use tempeh_runtime::{LatestTemperatureReadings, RealRunConfig, RealRunUpdate};

use crate::mqtt::MqttTelemetry;
use crate::status_led::{Status, StatusLed};
use crate::tasmota::TasmotaHeaterOutput;

const BUTTON_GPIO: i32 = 0;
const BOX_AIR_GPIO: i32 = 13;
const ROOM_AIR_GPIO: i32 = 6;
const PRODUCT_GPIO: i32 = 4;
const LOOP_DELAY_MS: u32 = 20;
const PROBE_SWEEP_INTERVAL_S: f32 = 2.0;
const PROBE_CONVERSION_TIME_S: f32 = 0.75;
const SAFETY_TICK_INTERVAL_S: f32 = 1.0;
const WIFI_RECONNECT_INTERVAL_S: f32 = 5.0;
const ACTUATOR_RECOVERY_INTERVAL_S: f32 = 5.0;
const LEASE_DURATION_S: u32 = 20;
const LEASE_RENEWAL_INTERVAL_S: f32 = 5.0;

const DS18B20_SKIP_ROM: u8 = 0xCC;
const DS18B20_CONVERT_T: u8 = 0x44;
const DS18B20_READ_SCRATCHPAD: u8 = 0xBE;
const WIFI_SSID: Option<&str> = option_env!("TEMPEH_WIFI_SSID");
const WIFI_PASSWORD: Option<&str> = option_env!("TEMPEH_WIFI_PASSWORD");
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

    let mut status_led = match StatusLed::new(peripherals.rmt.channel0, peripherals.pins.gpio48) {
        Ok(mut led) => {
            if let Err(error) = led.show(Status::Booting) {
                warn!("status LED unavailable; continuing with serial status: {error:#}");
                None
            } else {
                Some(led)
            }
        }
        Err(error) => {
            warn!("status LED unavailable; continuing with serial status: {error:#}");
            None
        }
    };
    let _button_pin = peripherals.pins.gpio0;

    if probes.box_air {
        let _box_air_pin = peripherals.pins.gpio13;
    }
    if probes.room_air {
        let _room_air_pin = peripherals.pins.gpio6;
    }
    if probes.product {
        let _product_pin = peripherals.pins.gpio4;
    }
    let modem = peripherals.modem;

    let mut wifi = connect_wifi(modem)?;
    let mut heater_output = TasmotaHeaterOutput::from_build_config()?;
    let mut latest = LatestTemperatureReadings::new();
    let mut supervisor = RunSupervisor::new(SupervisorConfig::new(
        RealRunConfig::default(),
        probes.product,
    ));
    let lease_config = LeaseConfig::new(LEASE_DURATION_S as f32, LEASE_RENEWAL_INTERVAL_S)
        .map_err(|error| anyhow!("invalid heater lease configuration: {error:?}"))?;
    let mut lease = HeaterLease::new(lease_config);
    let pulse_time = pulse_time_value_for_seconds(LEASE_DURATION_S)
        .context("lease duration cannot be represented by Tasmota PulseTime")?;
    let start_us = now_us();

    lease.poll(0.0);
    match heater_output.configure_fail_safe(pulse_time) {
        Ok(()) => {
            lease.record_success(0.0, false);
            supervisor.report_actuator_ready();
            println!("{}", format_state_line(0.0, "idle", "boot_configured"));
            print_actuator_line(0.0, &lease, "boot_configured");
        }
        Err(error) => {
            warn!("Tasmota boot configuration failed: {error:#}");
            lease.record_failure(0.0);
            let outcome = supervisor.report_fault(FaultReason::BootConfigFailed);
            print_supervisor_outcome(0.0, &outcome);
            print_actuator_line(0.0, &lease, "boot_config_failed");
        }
    }

    let button_input = ActiveLowButton::new(BUTTON_GPIO)?;
    let mut button = ButtonReader::new(ButtonConfig::default());
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

    info!("Tempeh OS autonomous ESP32 controller");
    info!("BOOT button GPIO{BUTTON_GPIO}: hold 2 s to start or acknowledge; press to stop");
    info!("status LED GPIO48: amber=boot, blue=idle, green=running, red=fault");
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
    info!("controller starts idle; heat requires a deliberate button hold");
    info!(
        "Tasmota lease: duration={LEASE_DURATION_S} s, renewal={LEASE_RENEWAL_INTERVAL_S} s, PulseTime={pulse_time}"
    );
    info!(
        "control output: control,time_s,room_air_temp_c,box_air_temp_c,product_temp_c,heater_on,reason"
    );

    let mut mqtt = match MqttTelemetry::from_build_config(probes) {
        Ok(telemetry) => telemetry,
        Err(error) => {
            warn!("MQTT unavailable; continuing without telemetry: {error:#}");
            None
        }
    };

    show_status(&mut status_led, supervisor.state());

    let mut box_air_conversion_started = false;
    let mut room_air_conversion_started = false;
    let mut product_conversion_started = false;
    let mut conversion_ready_at_s = None;
    let mut next_probe_sweep_s = 0.0_f32;
    let mut next_safety_tick_s = 0.0_f32;
    let mut next_wifi_reconnect_s = WIFI_RECONNECT_INTERVAL_S;
    let mut next_actuator_recovery_s = ACTUATOR_RECOVERY_INTERVAL_S;
    let mut last_diagnostics_s = 0.0_f32;
    let mut last_safety_tick_s = 0.0_f32;

    loop {
        let time_s = elapsed_s(start_us);
        let time_ms = elapsed_ms(start_us);

        maybe_reconnect_wifi(time_s, &mut next_wifi_reconnect_s, &mut wifi);

        if !supervisor.actuator_ready() && time_s >= next_actuator_recovery_s {
            next_actuator_recovery_s = time_s + ACTUATOR_RECOVERY_INTERVAL_S;
            match heater_output.configure_fail_safe(pulse_time) {
                Ok(()) => {
                    lease.set_desired(false, time_s);
                    lease.record_success(time_s, false);
                    supervisor.report_actuator_ready();
                    info!("Tasmota actuator recovered; fault acknowledgement is now permitted");
                    print_actuator_line(time_s, &lease, "actuator_recovered");
                }
                Err(error) => {
                    warn!("Tasmota actuator recovery failed: {error:#}");
                    lease.record_failure(time_s);
                    print_actuator_line(time_s, &lease, "actuator_recovery_failed");
                }
            }
        }

        if let Some(command) = button.poll(button_input.is_pressed(), time_ms, supervisor.state()) {
            let outcome = supervisor.handle_command(time_s, &latest, command);
            if outcome.state_change.is_none() {
                warn!(
                    "button command {command:?} rejected because safety conditions are not ready"
                );
            }
            print_supervisor_outcome(time_s, &outcome);
        }

        if conversion_ready_at_s.is_none() && time_s >= next_probe_sweep_s {
            box_air_conversion_started =
                start_probe_conversion(TemperatureProbe::BoxAir, &mut box_air);
            room_air_conversion_started =
                start_probe_conversion(TemperatureProbe::RoomAir, &mut room_air);
            product_conversion_started =
                start_probe_conversion(TemperatureProbe::Product, &mut product);

            if box_air_conversion_started
                || room_air_conversion_started
                || product_conversion_started
            {
                conversion_ready_at_s = Some(time_s + PROBE_CONVERSION_TIME_S);
            }
            next_probe_sweep_s = time_s + PROBE_SWEEP_INTERVAL_S;
        }

        if conversion_ready_at_s.is_some_and(|ready_at_s| time_s >= ready_at_s) {
            finish_probe_conversion(
                TemperatureProbe::BoxAir,
                &mut box_air,
                &mut box_air_conversion_started,
                time_s,
                &mut latest,
                &mut supervisor,
            );
            finish_probe_conversion(
                TemperatureProbe::RoomAir,
                &mut room_air,
                &mut room_air_conversion_started,
                time_s,
                &mut latest,
                &mut supervisor,
            );
            finish_probe_conversion(
                TemperatureProbe::Product,
                &mut product,
                &mut product_conversion_started,
                time_s,
                &mut latest,
                &mut supervisor,
            );
            conversion_ready_at_s = None;
        }

        if time_s >= next_safety_tick_s {
            let outcome = supervisor.tick(time_s, &latest, RealRunUpdate::Tick);
            print_supervisor_outcome(time_s, &outcome);
            last_safety_tick_s = time_s;
            next_safety_tick_s = time_s + SAFETY_TICK_INTERVAL_S;
        }

        lease.set_desired(supervisor.desired_heater_on(), time_s);
        run_heater_lease(time_s, &mut lease, &mut heater_output, &mut supervisor);
        show_status(&mut status_led, supervisor.state());
        if let Some(mqtt) = mqtt.as_mut() {
            mqtt.poll(
                time_s,
                &latest,
                supervisor.state(),
                supervisor.desired_heater_on(),
                lease.confirmed_heater_on(),
                supervisor.actuator_ready(),
            );
        }
        maybe_log_runtime_diagnostics(time_s, &mut last_diagnostics_s, last_safety_tick_s);

        FreeRtos::delay_ms(LOOP_DELAY_MS);
    }
}

fn maybe_reconnect_wifi(
    time_s: f32,
    next_attempt_s: &mut f32,
    wifi: &mut BlockingWifi<EspWifi<'static>>,
) {
    if time_s < *next_attempt_s {
        return;
    }
    *next_attempt_s = time_s + WIFI_RECONNECT_INTERVAL_S;

    match wifi.is_connected() {
        Ok(true) => {}
        Ok(false) => {
            warn!("Wi-Fi disconnected; requesting reconnect");
            if let Err(error) = wifi.wifi_mut().connect() {
                warn!("Wi-Fi reconnect request failed: {error}");
            }
        }
        Err(error) => warn!("failed to read Wi-Fi connection state: {error}"),
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

fn start_probe_conversion(probe_kind: TemperatureProbe, probe: &mut Option<Ds18b20>) -> bool {
    let Some(probe) = probe.as_mut() else {
        return false;
    };

    match probe.start_conversion() {
        Ok(()) => true,
        Err(error) => {
            warn!("{probe_kind:?} conversion start failed: {error:#}");
            false
        }
    }
}

fn finish_probe_conversion(
    probe_kind: TemperatureProbe,
    probe: &mut Option<Ds18b20>,
    conversion_started: &mut bool,
    time_s: f32,
    latest: &mut LatestTemperatureReadings,
    supervisor: &mut RunSupervisor,
) {
    if !*conversion_started {
        return;
    }
    *conversion_started = false;

    let Some(probe) = probe.as_mut() else {
        return;
    };

    match probe.read_conversion_c() {
        Ok(temp_c) => {
            println!("temp,{},{temp_c:.3}", probe_name(probe_kind));
            latest.update_at(time_s, probe_kind, temp_c);
            let outcome = supervisor.tick(time_s, latest, RealRunUpdate::Probe(probe_kind));
            print_supervisor_outcome(time_s, &outcome);
        }
        Err(error) => {
            warn!("{probe_kind:?} read failed: {error:#}");
        }
    }
}

fn run_heater_lease(
    time_s: f32,
    lease: &mut HeaterLease,
    heater_output: &mut TasmotaHeaterOutput,
    supervisor: &mut RunSupervisor,
) {
    if !supervisor.actuator_ready() {
        return;
    }

    let action = lease.poll(time_s);
    let (on, success_reason, failure_reason) = match action {
        LeaseAction::None => return,
        LeaseAction::SendOn => (true, "lease_renewed", "lease_renewal_failed"),
        LeaseAction::SendOff => (false, "safe_off_confirmed", "safe_off_failed"),
    };

    match heater_output.set_heater(on, success_reason) {
        Ok(()) => {
            lease.record_success(time_s, on);
            print_actuator_line(time_s, lease, success_reason);
        }
        Err(error) => {
            warn!("Tasmota heater command failed: {error:#}");
            if on {
                heater_output.attempt_fail_safe_off("lease_failure_safe_off");
            }

            lease.record_failure(time_s);
            print_actuator_line(time_s, lease, failure_reason);
            let outcome = supervisor.report_fault(FaultReason::ActuatorFailed);
            print_supervisor_outcome(time_s, &outcome);
            lease.set_desired(false, time_s);
        }
    }
}

fn print_supervisor_outcome(time_s: f32, outcome: &SupervisorOutcome) {
    if let Some(change) = outcome.state_change {
        println!(
            "{}",
            format_state_line(time_s, change.state.as_str(), change.reason)
        );
    }
    if let Some(sample) = outcome.sample.as_ref() {
        println!(
            "{}",
            format_control_line(
                sample.time_s,
                sample.room_air_temp_c,
                sample.box_air_temp_c,
                sample.product_temp_c,
                sample.heater_on,
                &sample.reason,
            )
        );
    }
}

fn print_actuator_line(time_s: f32, lease: &HeaterLease, reason: &str) {
    println!(
        "{}",
        format_actuator_line(
            time_s,
            lease.desired_heater_on(),
            lease.confirmed_heater_on(),
            reason,
        )
    );
}

fn show_status(status_led: &mut Option<StatusLed>, state: RunState) {
    let result = status_led
        .as_mut()
        .map(|status_led| status_led.show(state.into()));
    if let Some(Err(error)) = result {
        warn!("status LED failed; continuing with serial status: {error:#}");
        *status_led = None;
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

fn elapsed_ms(start_us: i64) -> u32 {
    let elapsed_us = now_us().saturating_sub(start_us);
    (elapsed_us / 1_000) as u32
}

struct ActiveLowButton {
    gpio: gpio_num_t,
}

impl ActiveLowButton {
    fn new(gpio: i32) -> Result<Self> {
        let button = Self { gpio };
        let config = gpio_config_t {
            pin_bit_mask: 1_u64 << button.gpio,
            mode: gpio_mode_t_GPIO_MODE_INPUT,
            pull_up_en: gpio_pullup_t_GPIO_PULLUP_ENABLE,
            pull_down_en: gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
            intr_type: gpio_int_type_t_GPIO_INTR_DISABLE,
        };
        esp!(unsafe { gpio_config(&config) }).context("failed to configure active-low button")?;
        Ok(button)
    }

    fn is_pressed(&self) -> bool {
        unsafe { gpio_get_level(self.gpio) == 0 }
    }
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

    fn start_conversion(&mut self) -> Result<()> {
        self.bus
            .reset()
            .context("DS18B20 did not respond to reset")?;
        self.bus.write_byte(DS18B20_SKIP_ROM)?;
        self.bus.write_byte(DS18B20_CONVERT_T)?;
        Ok(())
    }

    fn read_conversion_c(&mut self) -> Result<f32> {
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
