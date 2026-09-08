use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use embedded_svc::http::client::Client;
use embedded_svc::utils::io;
use esp_idf_svc::http::client::{Configuration as HttpConfiguration, EspHttpConnection};
use log::{info, warn};
use tempeh_protocol::tasmota::{
    TasmotaResponseError, normalise_base_url, parse_power_on_state_response, parse_power_response,
    parse_pulse_time_response, power_command_url, power_on_state_command_url,
    pulse_time_command_url,
};

const TASMOTA_BASE_URL: Option<&str> = option_env!("TEMPEH_TASMOTA_BASE_URL");
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const RESPONSE_BUFFER_BYTES: usize = 512;

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

        let result = self.request(&url, parse_power_response).and_then(|reply| {
            if reply.confirms(on) {
                Ok(())
            } else {
                bail!("Tasmota reported a contradictory power state")
            }
        });

        match result {
            Ok(()) => {
                info!("Tasmota heater command confirmed: heater_on={on}");
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    pub fn attempt_fail_safe_off(&mut self, reason: &str) {
        if let Err(error) = self.set_heater(false, reason) {
            warn!("fail-safe Tasmota off command failed; actuator state is unknown: {error:#}");
        }
    }

    fn set_pulse_time(&self, pulse_time: u32) -> Result<()> {
        let url = pulse_time_command_url(&self.base_url, pulse_time);
        let reply = self.request(&url, parse_pulse_time_response)?;
        if !reply.confirms(pulse_time) {
            bail!(
                "Tasmota confirmed PulseTime {} instead of {pulse_time}",
                reply.set
            );
        }
        info!("Tasmota PulseTime confirmed: value={pulse_time}");
        Ok(())
    }

    fn set_power_on_state(&self, power_on_state: u8) -> Result<()> {
        let url = power_on_state_command_url(&self.base_url, power_on_state);
        let reply = self.request(&url, parse_power_on_state_response)?;
        if !reply.confirms(power_on_state) {
            bail!(
                "Tasmota confirmed PowerOnState {} instead of {power_on_state}",
                reply.state
            );
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
            .context("failed to create ESP HTTP connection")?;
        let mut client = Client::wrap(connection);
        let request = client
            .get(url)
            .context("failed to create Tasmota request")?;
        let mut response = request.submit().context("failed to send Tasmota request")?;

        let status = response.status();
        if !(200..300).contains(&status) {
            bail!("Tasmota heater command failed with HTTP status {status}");
        }

        let mut buffer = [0_u8; RESPONSE_BUFFER_BYTES];
        let bytes_read =
            io::try_read_full(&mut response, &mut buffer).map_err(|(error, bytes_read)| {
                anyhow!("failed to read Tasmota response after {bytes_read} bytes: {error}")
            })?;
        if bytes_read == buffer.len() {
            bail!("Tasmota response exceeds {RESPONSE_BUFFER_BYTES} bytes");
        }

        let body = std::str::from_utf8(&buffer[..bytes_read])
            .context("Tasmota response was not valid UTF-8")?;
        parse(body).map_err(|error| anyhow!("invalid Tasmota response: {error:?}; body={body:?}"))
    }
}
