use std::time::{Duration, Instant};
use std::{error::Error, fmt};

use anyhow::{Context, Result, anyhow, bail};
use embedded_svc::http::client::Client;
use embedded_svc::utils::io;
use esp_idf_svc::http::client::{Configuration as HttpConfiguration, EspHttpConnection};
use log::info;
use tempeh_protocol::tasmota::{
    TasmotaResponseError, normalise_base_url, parse_power_on_state_response, parse_power_response,
    parse_pulse_time_response, power_command_url, power_on_state_command_url,
    pulse_time_command_url,
};

const TASMOTA_BASE_URL: Option<&str> = option_env!("TEMPEH_TASMOTA_BASE_URL");
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const RESPONSE_BUFFER_BYTES: usize = 512;

#[derive(Debug)]
pub struct ContradictoryPowerState;

impl fmt::Display for ContradictoryPowerState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Tasmota reported a contradictory power state")
    }
}

impl Error for ContradictoryPowerState {}

#[derive(Debug)]
pub struct RejectedSafetySetting(pub &'static str, pub u32, pub u32);

impl fmt::Display for RejectedSafetySetting {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Tasmota confirmed {}={} instead of {}",
            self.0, self.2, self.1
        )
    }
}

impl Error for RejectedSafetySetting {}

#[derive(Debug, Clone)]
pub struct TasmotaHeaterOutput {
    base_url: String,
}

impl TasmotaHeaterOutput {
    pub fn from_build_config() -> Result<Self> {
        let base_url = TASMOTA_BASE_URL.unwrap_or_default();

        if base_url.is_empty() {
            bail!(
                "TEMPEH_TASMOTA_BASE_URL is not set. Add [tasmota].base_url to firmware.local.toml"
            );
        }

        Ok(Self::new(base_url))
    }

    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: normalise_base_url(base_url),
        }
    }

    pub fn configure_fail_safe(&mut self, pulse_time: u32) -> Result<()> {
        self.set_heater(false, "boot_safe_off")?;
        self.set_power_on_state(0)?;
        self.set_pulse_time(pulse_time)?;
        Ok(())
    }

    pub fn set_heater(&mut self, on: bool, reason: &str) -> Result<()> {
        let url = power_command_url(&self.base_url, on);
        let command_label = if on { "on" } else { "off" };

        info!("sending Tasmota heater {command_label} command: reason={reason}");

        let started = Instant::now();
        let result = self.request(&url, parse_power_response).and_then(|reply| {
            if reply.confirms(on) {
                Ok(())
            } else {
                Err(ContradictoryPowerState.into())
            }
        });

        match result {
            Ok(()) => {
                info!(
                    "Tasmota heater command confirmed: heater_on={on}, elapsed_ms={}",
                    started.elapsed().as_millis()
                );
                Ok(())
            }
            Err(error) => Err(error).with_context(|| {
                format!(
                    "Tasmota heater {command_label} elapsed_ms={}",
                    started.elapsed().as_millis()
                )
            }),
        }
    }

    fn set_pulse_time(&self, pulse_time: u32) -> Result<()> {
        let url = pulse_time_command_url(&self.base_url, pulse_time);
        let reply = self.request(&url, parse_pulse_time_response)?;
        if !reply.confirms(pulse_time) {
            return Err(RejectedSafetySetting("PulseTime", pulse_time, reply.set).into());
        }
        info!("Tasmota PulseTime confirmed: value={pulse_time}");
        Ok(())
    }

    fn set_power_on_state(&self, power_on_state: u8) -> Result<()> {
        let url = power_on_state_command_url(&self.base_url, power_on_state);
        let reply = self.request(&url, parse_power_on_state_response)?;
        if !reply.confirms(power_on_state) {
            return Err(RejectedSafetySetting(
                "PowerOnState",
                power_on_state.into(),
                reply.state.into(),
            )
            .into());
        }
        info!("Tasmota PowerOnState confirmed: value={power_on_state}");
        Ok(())
    }

    fn request<T>(
        &self,
        url: &str,
        parse: impl FnOnce(&str) -> std::result::Result<T, TasmotaResponseError>,
    ) -> Result<T> {
        let configuration = HttpConfiguration {
            timeout: Some(REQUEST_TIMEOUT),
            ..Default::default()
        };

        let connection = EspHttpConnection::new(&configuration)
            .context("phase=connection_setup: failed to create ESP HTTP connection")?;
        let mut client = Client::wrap(connection);
        let request = client
            .get(url)
            .context("phase=request_setup: failed to create Tasmota request")?;
        let mut response = request
            .submit()
            .context("phase=submit: failed to send Tasmota request")?;

        let status = response.status();
        if !(200..300).contains(&status) {
            bail!("phase=http_status: Tasmota heater command failed with HTTP status {status}");
        }

        let mut buffer = [0_u8; RESPONSE_BUFFER_BYTES];
        let bytes_read =
            io::try_read_full(&mut response, &mut buffer).map_err(|(error, bytes_read)| {
                anyhow!("phase=response_read: failed to read Tasmota response after {bytes_read} bytes: {error}")
            })?;
        if bytes_read == buffer.len() {
            bail!("phase=response_read: Tasmota response exceeds {RESPONSE_BUFFER_BYTES} bytes");
        }

        let body = std::str::from_utf8(&buffer[..bytes_read])
            .context("phase=response_parse: Tasmota response was not valid UTF-8")?;
        parse(body).map_err(|error| {
            anyhow!("phase=response_parse: invalid Tasmota response: {error:?}; body={body:?}")
        })
    }
}
