use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use esp_idf_svc::mqtt::client::{
    EspMqttClient, EventPayload, LwtConfiguration, MqttClientConfiguration, QoS,
};
use log::{info, warn};
use tempeh_protocol::home_assistant::discovery_messages;
use tempeh_protocol::mqtt::{
    EventBuffer, StatePayload, availability_topic, event_payload, event_topic, state_payload,
    state_topic, validate_device_id,
};
use tempeh_runtime::LatestTemperatureReadings;
use tempeh_runtime::heater_lease::HeaterLease;
use tempeh_runtime::run_supervisor::RunState;

use crate::ProbeConfig;

const MQTT_BROKER_URL: Option<&str> = option_env!("TEMPEH_MQTT_BROKER_URL");
const MQTT_USERNAME: Option<&str> = option_env!("TEMPEH_MQTT_USERNAME");
const MQTT_PASSWORD: Option<&str> = option_env!("TEMPEH_MQTT_PASSWORD");
const MQTT_DEVICE_ID: Option<&str> = option_env!("TEMPEH_MQTT_DEVICE_ID");
const MQTT_DEVICE_NAME: Option<&str> = option_env!("TEMPEH_MQTT_DEVICE_NAME");
const MQTT_HOME_ASSISTANT_DISCOVERY: Option<&str> =
    option_env!("TEMPEH_MQTT_HOME_ASSISTANT_DISCOVERY");

const DEFAULT_DEVICE_ID: &str = "tempeh_controller";
const DEFAULT_DEVICE_NAME: &str = "Tempeh Controller";
const TELEMETRY_INTERVAL_S: f32 = 10.0;
const PUBLISH_RETRY_INTERVAL_S: f32 = 5.0;
const MAX_EVENTS_PER_POLL: usize = 4;

pub struct MqttTelemetry {
    client: EspMqttClient<'static>,
    connection: Arc<ConnectionState>,
    observed_generation: u32,
    device_id: &'static str,
    device_name: &'static str,
    probes: ProbeConfig,
    home_assistant_discovery: bool,
    next_periodic_publish_s: f32,
    next_publish_attempt_s: f32,
    last_status: Option<TelemetryStatus>,
    events: EventBuffer,
}

impl MqttTelemetry {
    pub fn from_build_config(
        probes: ProbeConfig,
        boot_id: &str,
        initial_state: RunState,
    ) -> Result<Option<Self>> {
        let Some(broker_url) = non_empty(MQTT_BROKER_URL) else {
            info!("MQTT telemetry disabled: no [mqtt] broker_url configured");
            return Ok(None);
        };

        let username = non_empty(MQTT_USERNAME);
        let password = non_empty(MQTT_PASSWORD);
        if username.is_some() != password.is_some() {
            bail!("MQTT requires both username and password, or neither");
        }

        let device_id = non_empty(MQTT_DEVICE_ID).unwrap_or(DEFAULT_DEVICE_ID);
        let device_name = non_empty(MQTT_DEVICE_NAME).unwrap_or(DEFAULT_DEVICE_NAME);
        let home_assistant_discovery = MQTT_HOME_ASSISTANT_DISCOVERY != Some("false");
        validate_device_id(device_id)
            .map_err(|error| anyhow!("invalid MQTT device_id: {error:?}"))?;

        let availability_topic = availability_topic(device_id)
            .map_err(|error| anyhow!("invalid MQTT availability topic: {error:?}"))?;
        let connection = Arc::new(ConnectionState::default());
        let callback_connection = Arc::clone(&connection);
        let configuration = MqttClientConfiguration {
            client_id: Some(device_id),
            username,
            password,
            keep_alive_interval: Some(Duration::from_secs(15)),
            reconnect_timeout: Some(Duration::from_secs(5)),
            network_timeout: Duration::from_secs(2),
            lwt: Some(LwtConfiguration {
                topic: &availability_topic,
                payload: b"offline",
                qos: QoS::AtLeastOnce,
                retain: true,
            }),
            ..Default::default()
        };

        let client = EspMqttClient::new_cb(broker_url, &configuration, move |event| {
            match event.payload() {
                EventPayload::Connected(_) => {
                    callback_connection.connected.store(true, Ordering::Release);
                    callback_connection
                        .generation
                        .fetch_add(1, Ordering::AcqRel);
                    info!("MQTT connected");
                }
                EventPayload::BeforeConnect | EventPayload::Disconnected => {
                    callback_connection
                        .connected
                        .store(false, Ordering::Release);
                }
                EventPayload::Error(error) => {
                    callback_connection
                        .connected
                        .store(false, Ordering::Release);
                    warn!("MQTT connection error: {error:?}");
                }
                _ => {}
            }
        })
        .with_context(|| format!("failed to create MQTT client for {broker_url}"))?;

        info!(
            "MQTT telemetry starting: broker={broker_url}, device_id={device_id}, home_assistant_discovery={home_assistant_discovery}"
        );
        Ok(Some(Self {
            client,
            connection,
            observed_generation: 0,
            device_id,
            device_name,
            probes,
            home_assistant_discovery,
            next_periodic_publish_s: 0.0,
            next_publish_attempt_s: 0.0,
            last_status: None,
            events: EventBuffer::new(boot_id.to_owned(), 0.0, fault_reason(initial_state)),
        }))
    }

    pub fn observe_state_change(&mut self, time_s: f32, state: RunState) {
        let dropped = self.events.observe_fault(time_s, fault_reason(state));
        if dropped != 0 {
            warn!("MQTT event buffer full; discarded {dropped} oldest event(s)");
        }
    }

    pub fn poll(
        &mut self,
        time_s: f32,
        latest: &LatestTemperatureReadings,
        run_state: RunState,
        desired_heater_on: bool,
        lease: &HeaterLease,
        actuator_ready: bool,
        interruption_started_s: Option<f32>,
    ) {
        if !self.connection.connected.load(Ordering::Acquire)
            || time_s < self.next_publish_attempt_s
        {
            return;
        }

        let generation = self.connection.generation.load(Ordering::Acquire);
        let new_connection = generation != self.observed_generation;
        let status = TelemetryStatus {
            run_state,
            desired_heater_on,
            confirmed_heater_on: lease.current_confirmation(time_s),
            actuator_ready,
            interruption_started_s,
        };
        let status_changed = self.last_status != Some(status);
        let periodic_due = time_s >= self.next_periodic_publish_s;

        let result = (|| -> Result<()> {
            if new_connection {
                self.publish_online()?;
                if self.home_assistant_discovery {
                    self.publish_home_assistant_discovery()?;
                }
            }

            if new_connection || status_changed || periodic_due {
                self.publish_state(
                    time_s,
                    latest,
                    run_state,
                    desired_heater_on,
                    lease,
                    actuator_ready,
                    interruption_started_s,
                )?;
            }

            self.publish_pending_events()?;

            Ok(())
        })();

        match result {
            Ok(()) => {
                self.observed_generation = generation;
                self.last_status = Some(status);
                if new_connection || status_changed || periodic_due {
                    self.next_periodic_publish_s = time_s + TELEMETRY_INTERVAL_S;
                }
            }
            Err(error) => {
                warn!("MQTT publish failed: {error:#}");
                self.next_publish_attempt_s = time_s + PUBLISH_RETRY_INTERVAL_S;
            }
        }
    }

    fn publish_online(&mut self) -> Result<()> {
        let topic = availability_topic(self.device_id)
            .map_err(|error| anyhow!("invalid MQTT availability topic: {error:?}"))?;
        self.enqueue(&topic, b"online", true)
    }

    fn publish_home_assistant_discovery(&mut self) -> Result<()> {
        let messages = discovery_messages(
            self.device_id,
            self.device_name,
            self.probes.room_air,
            self.probes.product,
        )
        .map_err(|error| anyhow!("failed to build Home Assistant discovery: {error:?}"))?;

        for message in messages {
            self.enqueue(&message.topic, message.payload.as_bytes(), true)?;
        }
        info!("Home Assistant MQTT discovery published");
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn publish_state(
        &mut self,
        time_s: f32,
        latest: &LatestTemperatureReadings,
        run_state: RunState,
        desired_heater_on: bool,
        lease: &HeaterLease,
        actuator_ready: bool,
        interruption_started_s: Option<f32>,
    ) -> Result<()> {
        let payload = state_payload(StatePayload {
            time_s,
            boot_id: self.events.boot_id(),
            room_air_temp_c: latest.room_air_temp_c,
            room_air_age_s: age_since(time_s, latest.room_air_updated_at_s),
            box_air_temp_c: latest.box_air_temp_c,
            box_air_age_s: age_since(time_s, latest.box_air_updated_at_s),
            product_temp_c: latest.product_temp_c,
            product_age_s: age_since(time_s, latest.product_updated_at_s),
            run_state: run_state.as_str(),
            fault_reason: fault_reason(run_state),
            pause_reason: (run_state == RunState::Paused).then_some("actuator_unreachable"),
            actuator_warning: match run_state {
                RunState::Running if interruption_started_s.is_some() => Some("on_reply_missing"),
                RunState::Idle if !actuator_ready => Some("configuration_unconfirmed"),
                _ => None,
            },
            interruption_started_s: interruption_started_s,
            desired_heater_on,
            confirmed_heater_on: lease.current_confirmation(time_s),
            last_confirmed_heater_on: lease.last_successful_state(),
            confirmation_age_s: age_since(time_s, lease.last_successful_command_s()),
            lease_duration_s: lease.lease_duration_s(),
            actuator_ready,
        })
        .map_err(|error| anyhow!("failed to build MQTT state: {error:?}"))?;
        let topic = state_topic(self.device_id)
            .map_err(|error| anyhow!("invalid MQTT state topic: {error:?}"))?;

        self.enqueue(&topic, payload.as_bytes(), true)
    }

    fn publish_pending_events(&mut self) -> Result<()> {
        let topic = event_topic(self.device_id)
            .map_err(|error| anyhow!("invalid MQTT event topic: {error:?}"))?;
        for _ in 0..MAX_EVENTS_PER_POLL {
            let Some(event) = self.events.front() else {
                break;
            };
            let payload = event_payload(self.events.boot_id(), event)
                .map_err(|error| anyhow!("failed to build MQTT event: {error:?}"))?;
            self.enqueue(&topic, payload.as_bytes(), false)?;
            self.events.pop_front();
        }
        Ok(())
    }

    fn enqueue(&mut self, topic: &str, payload: &[u8], retain: bool) -> Result<()> {
        self.client
            .enqueue(topic, QoS::AtLeastOnce, retain, payload)
            .with_context(|| format!("failed to enqueue MQTT topic {topic}"))?;
        Ok(())
    }
}

fn fault_reason(state: RunState) -> Option<&'static str> {
    match state {
        RunState::Fault(reason) => Some(reason.as_str()),
        RunState::Idle | RunState::Running | RunState::Paused => None,
    }
}

fn age_since(time_s: f32, updated_at_s: Option<f32>) -> Option<f32> {
    updated_at_s.map(|at| (time_s - at).max(0.0))
}

fn non_empty(value: Option<&'static str>) -> Option<&'static str> {
    value.filter(|value| !value.trim().is_empty())
}

#[derive(Debug, Default)]
struct ConnectionState {
    connected: AtomicBool,
    generation: AtomicU32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TelemetryStatus {
    run_state: RunState,
    desired_heater_on: bool,
    confirmed_heater_on: Option<bool>,
    actuator_ready: bool,
    interruption_started_s: Option<f32>,
}
