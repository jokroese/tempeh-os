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
    esp, esp_get_free_heap_size, esp_get_minimum_free_heap_size, esp_random, esp_timer_get_time,
    gpio_config, gpio_config_t, gpio_get_level, gpio_int_type_t_GPIO_INTR_DISABLE,
    gpio_mode_t_GPIO_MODE_INPUT, gpio_mode_t_GPIO_MODE_INPUT_OUTPUT_OD, gpio_num_t,
    gpio_pulldown_t_GPIO_PULLDOWN_DISABLE, gpio_pullup_t_GPIO_PULLUP_ENABLE, gpio_set_level,
    uxTaskGetStackHighWaterMark, xTaskGetCurrentTaskHandle,
};
use log::{info, warn};
use tempeh_model::TemperatureProbe;
use tempeh_protocol::status::{ProbeStatus, STATUS_VERSION, StatusRecord, format_status_line};
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
use crate::tasmota::{ContradictoryPowerState, RejectedSafetySetting, TasmotaHeaterOutput};

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
const STATUS_INTERVAL_S: f32 = 5.0;

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
            product: parse_probe_bool(PROBE_PRODUCT, false),
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
    let boot_id = format!("{:08x}{:08x}", unsafe { esp_random() }, unsafe {
        esp_random()
    });

    lease.poll(0.0);
    match heater_output.configure_fail_safe(pulse_time) {
        Ok(()) => {
            lease.record_success(0.0, false);
            supervisor.report_actuator_ready();
            println!("{}", format_state_line(0.0, "idle", "boot_configured"));
            print_actuator_line(elapsed_s(start_us), &lease, "boot_configured");
        }
        Err(error) => {
            warn!("Tasmota boot configuration failed: {error:#}");
            lease.record_failure(0.0);
            if error.downcast_ref::<ContradictoryPowerState>().is_some()
                || error.downcast_ref::<RejectedSafetySetting>().is_some()
            {
                let outcome = supervisor.report_fault(FaultReason::BootConfigFailed);
                print_supervisor_outcome(0.0, &outcome);
            }
            print_actuator_line(elapsed_s(start_us), &lease, "boot_config_failed");
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
    info!(
        "status LED GPIO48: amber=boot/retrying, blue=idle, green=running, purple=paused, red=fault"
    );
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

    let mut mqtt = match MqttTelemetry::from_build_config(probes, &boot_id, supervisor.state()) {
        Ok(telemetry) => telemetry,
        Err(error) => {
            warn!("MQTT unavailable; continuing without telemetry: {error:#}");
            None
        }
    };

    show_status(
        &mut status_led,
        supervisor.state(),
        !supervisor.actuator_ready(),
    );

    let mut box_air_conversion_started = false;
    let mut room_air_conversion_started = false;
    let mut product_conversion_started = false;
    let mut conversion_ready_at_s = None;
    let mut next_probe_sweep_s = 0.0_f32;
    let mut next_safety_tick_s = 0.0_f32;
    let mut next_wifi_reconnect_s = WIFI_RECONNECT_INTERVAL_S;
    let mut wifi_was_connected = true;
    let mut next_actuator_recovery_s = ACTUATOR_RECOVERY_INTERVAL_S;
    let mut interruption_started_s: Option<f32> = None;
    let mut last_diagnostics_s = 0.0_f32;
    let mut last_safety_tick_s = 0.0_f32;
    let mut next_status_s = 0.0_f32;
    let mut last_reported_state = None;

    loop {
        let time_s = elapsed_s(start_us);
        let time_ms = elapsed_ms(start_us);

        maybe_reconnect_wifi(
            time_s,
            &mut next_wifi_reconnect_s,
            &mut wifi_was_connected,
            &mut wifi,
        );

        if !supervisor.actuator_ready() && time_s >= next_actuator_recovery_s {
            next_actuator_recovery_s = time_s + ACTUATOR_RECOVERY_INTERVAL_S;
            match heater_output.configure_fail_safe(pulse_time) {
                Ok(()) => {
                    let confirmed_at_s = elapsed_s(start_us);
                    lease.set_desired(false, confirmed_at_s);
                    lease.record_success(confirmed_at_s, false);
                    supervisor.report_actuator_ready();
                    print_actuator_line(confirmed_at_s, &lease, "actuator_recovered");
                    if supervisor.state() == RunState::Paused {
                        match supervisor.paused_heater_demand(confirmed_at_s, &latest) {
                            Ok(needs_heat) => {
                                let on_confirmed = if needs_heat {
                                    let attempt_s = elapsed_s(start_us);
                                    match heater_output.set_heater(true, "paused_auto_resume") {
                                        Ok(()) => {
                                            let now_s = elapsed_s(start_us);
                                            lease.set_desired(true, now_s);
                                            lease.record_success(now_s, true);
                                            print_actuator_line(
                                                now_s,
                                                &lease,
                                                "resume_on_confirmed",
                                            );
                                            true
                                        }
                                        Err(error) => {
                                            warn!("Tasmota resume ON not confirmed: {error:#}");
                                            let now_s = elapsed_s(start_us);
                                            lease.record_ambiguous_on_failure(now_s);
                                            interruption_started_s.get_or_insert(attempt_s);
                                            supervisor.report_actuator_unready();
                                            next_actuator_recovery_s =
                                                now_s + ACTUATOR_RECOVERY_INTERVAL_S;
                                            print_actuator_line(
                                                now_s,
                                                &lease,
                                                "resume_on_unconfirmed",
                                            );
                                            if error
                                                .downcast_ref::<ContradictoryPowerState>()
                                                .is_some()
                                            {
                                                let outcome = supervisor
                                                    .report_fault(FaultReason::ActuatorFailed);
                                                report_supervisor_outcome(
                                                    now_s, &outcome, &mut mqtt,
                                                );
                                            }
                                            false
                                        }
                                    }
                                } else {
                                    true
                                };
                                if on_confirmed {
                                    let now_s = elapsed_s(start_us);
                                    let outcome =
                                        supervisor.resume_after_actuator_recovery(now_s, &latest);
                                    report_supervisor_outcome(now_s, &outcome, &mut mqtt);
                                    if let Some(started_s) = interruption_started_s {
                                        info!(
                                            "actuator communication restored after {:.1} s",
                                            now_s - started_s
                                        );
                                    }
                                    interruption_started_s = None;
                                }
                            }
                            Err(reason) => {
                                let outcome = supervisor.report_fault(reason);
                                report_supervisor_outcome(confirmed_at_s, &outcome, &mut mqtt);
                            }
                        }
                    } else {
                        info!("Tasmota actuator recovered");
                        interruption_started_s = None;
                    }
                }
                Err(error) => {
                    warn!("Tasmota actuator recovery failed: {error:#}");
                    let now_s = elapsed_s(start_us);
                    lease.record_failure(now_s);
                    print_actuator_line(now_s, &lease, "actuator_recovery_failed");
                    if error.downcast_ref::<ContradictoryPowerState>().is_some()
                        || error.downcast_ref::<RejectedSafetySetting>().is_some()
                    {
                        let outcome = supervisor.report_fault(FaultReason::ActuatorFailed);
                        report_supervisor_outcome(now_s, &outcome, &mut mqtt);
                    }
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
            report_supervisor_outcome(time_s, &outcome, &mut mqtt);
            if outcome.state == RunState::Idle {
                interruption_started_s = None;
            }
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
                &mut mqtt,
            );
            finish_probe_conversion(
                TemperatureProbe::RoomAir,
                &mut room_air,
                &mut room_air_conversion_started,
                time_s,
                &mut latest,
                &mut supervisor,
                &mut mqtt,
            );
            finish_probe_conversion(
                TemperatureProbe::Product,
                &mut product,
                &mut product_conversion_started,
                time_s,
                &mut latest,
                &mut supervisor,
                &mut mqtt,
            );
            conversion_ready_at_s = None;
        }

        if time_s >= next_safety_tick_s {
            let outcome = supervisor.tick(time_s, &latest, RealRunUpdate::Tick);
            report_supervisor_outcome(time_s, &outcome, &mut mqtt);
            last_safety_tick_s = time_s;
            next_safety_tick_s = time_s + SAFETY_TICK_INTERVAL_S;
        }

        if supervisor.state() == RunState::Running
            && interruption_started_s.is_some()
            && lease.on_retry_window_expired(elapsed_s(start_us))
        {
            let now_s = elapsed_s(start_us);
            let outcome = supervisor.pause_for_actuator();
            report_supervisor_outcome(now_s, &outcome, &mut mqtt);
            lease.set_desired(false, now_s);
            next_actuator_recovery_s = now_s;
        }

        lease.set_desired(supervisor.desired_heater_on(), time_s);
        run_heater_lease(
            time_s,
            start_us,
            &mut lease,
            &mut heater_output,
            &mut supervisor,
            &mut mqtt,
            &mut interruption_started_s,
            &mut next_actuator_recovery_s,
        );
        show_status(
            &mut status_led,
            supervisor.state(),
            interruption_started_s.is_some() || !supervisor.actuator_ready(),
        );
        let report_time_s = elapsed_s(start_us);
        if report_time_s >= next_status_s || last_reported_state != Some(supervisor.state()) {
            print_status(
                &boot_id,
                report_time_s,
                &latest,
                &supervisor,
                &lease,
                interruption_started_s,
            );
            last_reported_state = Some(supervisor.state());
            next_status_s = report_time_s + STATUS_INTERVAL_S;
        }
        if let Some(mqtt) = mqtt.as_mut() {
            mqtt.poll(
                report_time_s,
                &latest,
                supervisor.state(),
                supervisor.desired_heater_on(),
                &lease,
                supervisor.actuator_ready(),
                interruption_started_s,
            );
        }
        maybe_log_runtime_diagnostics(time_s, &mut last_diagnostics_s, last_safety_tick_s);

        FreeRtos::delay_ms(LOOP_DELAY_MS);
    }
}

fn maybe_reconnect_wifi(
    time_s: f32,
    next_attempt_s: &mut f32,
    was_connected: &mut bool,
    wifi: &mut BlockingWifi<EspWifi<'static>>,
) {
    if time_s < *next_attempt_s {
        return;
    }
    *next_attempt_s = time_s + WIFI_RECONNECT_INTERVAL_S;

    match wifi.is_connected() {
        Ok(true) => {
            if !*was_connected {
                info!("Wi-Fi reconnected");
            }
            *was_connected = true;
        }
        Ok(false) => {
            *was_connected = false;
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
    mqtt: &mut Option<MqttTelemetry>,
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
            report_supervisor_outcome(time_s, &outcome, mqtt);
        }
        Err(error) => {
            warn!("{probe_kind:?} read failed: {error:#}");
        }
    }
}

fn run_heater_lease(
    time_s: f32,
    start_us: i64,
    lease: &mut HeaterLease,
    heater_output: &mut TasmotaHeaterOutput,
    supervisor: &mut RunSupervisor,
    mqtt: &mut Option<MqttTelemetry>,
    interruption_started_s: &mut Option<f32>,
    next_actuator_recovery_s: &mut f32,
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

    let was_confirmed_off = lease.current_confirmation(time_s) == Some(false);
    let attempt_started_s = elapsed_s(start_us);

    match heater_output.set_heater(on, success_reason) {
        Ok(()) => {
            let now_s = elapsed_s(start_us);
            lease.record_success(now_s, on);
            if on {
                *interruption_started_s = None;
            }
            print_actuator_line(now_s, lease, success_reason);
        }
        Err(error) => {
            warn!("Tasmota heater command failed: {error:#}");
            let now_s = elapsed_s(start_us);
            if on {
                if error.downcast_ref::<ContradictoryPowerState>().is_some() {
                    lease.record_failure(now_s);
                    let outcome = supervisor.report_fault(FaultReason::ActuatorFailed);
                    report_supervisor_outcome(now_s, &outcome, mqtt);
                    lease.set_desired(false, now_s);
                    *next_actuator_recovery_s = now_s;
                    print_actuator_line(now_s, lease, "contradictory_on_reply");
                    return;
                }

                lease.record_ambiguous_on_failure(now_s);
                interruption_started_s.get_or_insert(attempt_started_s);
                print_actuator_line(now_s, lease, "renewal_reply_missing");
                if lease.on_retry_window_expired(now_s) {
                    let outcome = supervisor.pause_for_actuator();
                    report_supervisor_outcome(now_s, &outcome, mqtt);
                    lease.set_desired(false, now_s);
                    *next_actuator_recovery_s = now_s;
                }
            } else {
                lease.record_failure(now_s);
                print_actuator_line(now_s, lease, failure_reason);
                if error.downcast_ref::<ContradictoryPowerState>().is_some() {
                    let outcome = supervisor.report_fault(FaultReason::ActuatorFailed);
                    report_supervisor_outcome(now_s, &outcome, mqtt);
                    lease.set_desired(false, now_s);
                    *next_actuator_recovery_s = now_s;
                } else if !was_confirmed_off {
                    interruption_started_s.get_or_insert(attempt_started_s);
                    let outcome = supervisor.pause_for_actuator();
                    report_supervisor_outcome(now_s, &outcome, mqtt);
                    supervisor.report_actuator_unready();
                    lease.set_desired(false, now_s);
                    *next_actuator_recovery_s = now_s;
                }
            }
        }
    }
}

fn report_supervisor_outcome(
    time_s: f32,
    outcome: &SupervisorOutcome,
    mqtt: &mut Option<MqttTelemetry>,
) {
    print_supervisor_outcome(time_s, outcome);
    if let (Some(change), Some(mqtt)) = (outcome.state_change, mqtt.as_mut()) {
        mqtt.observe_state_change(time_s, change.state);
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
            lease.current_confirmation(time_s),
            reason,
        )
    );
}

fn print_status(
    boot_id: &str,
    time_s: f32,
    latest: &LatestTemperatureReadings,
    supervisor: &RunSupervisor,
    lease: &HeaterLease,
    interruption_started_s: Option<f32>,
) {
    fn probe(value: Option<f32>, updated_at_s: Option<f32>, now_s: f32) -> ProbeStatus {
        ProbeStatus {
            temp_c: value,
            age_s: updated_at_s.map(|at| (now_s - at).max(0.0)),
        }
    }

    let fault_reason = match supervisor.state() {
        RunState::Fault(reason) => Some(reason.as_str().to_owned()),
        RunState::Idle | RunState::Running | RunState::Paused => None,
    };
    let status = StatusRecord {
        version: STATUS_VERSION,
        boot_id: boot_id.to_owned(),
        uptime_s: time_s,
        run_state: supervisor.state().as_str().to_owned(),
        fault_reason,
        pause_reason: (supervisor.state() == RunState::Paused)
            .then(|| "actuator_unreachable".to_owned()),
        actuator_warning: match supervisor.state() {
            RunState::Running if interruption_started_s.is_some() => {
                Some("on_reply_missing".to_owned())
            }
            RunState::Idle if !supervisor.actuator_ready() => {
                Some("configuration_unconfirmed".to_owned())
            }
            _ => None,
        },
        interruption_started_s: interruption_started_s,
        desired_heater_on: supervisor.desired_heater_on(),
        actuator_ready: supervisor.actuator_ready(),
        confirmed_heater_on: lease.current_confirmation(time_s),
        last_confirmed_heater_on: lease.last_successful_state(),
        last_successful_command_s: lease.last_successful_command_s(),
        last_successful_renewal_s: lease.last_successful_renewal_s(),
        lease_duration_s: lease.lease_duration_s(),
        room_air: probe(latest.room_air_temp_c, latest.room_air_updated_at_s, time_s),
        box_air: probe(latest.box_air_temp_c, latest.box_air_updated_at_s, time_s),
        product: probe(latest.product_temp_c, latest.product_updated_at_s, time_s),
    };
    println!("{}", format_status_line(&status));
}

fn show_status(status_led: &mut Option<StatusLed>, state: RunState, retrying: bool) {
    let status = if matches!(state, RunState::Idle | RunState::Running) && retrying {
        Status::Retrying
    } else {
        state.into()
    };
    let result = status_led
        .as_mut()
        .map(|status_led| status_led.show(status));
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
