use anyhow::{Context, Result, bail};
use embedded_svc::http::client::Client;
use esp_idf_svc::http::client::{Configuration as HttpConfiguration, EspHttpConnection};
use log::{info, warn};
use tempeh_protocol::tasmota::{normalise_base_url, power_command_url};

const TASMOTA_BASE_URL: Option<&str> = option_env!("TEMPEH_TASMOTA_BASE_URL");

#[derive(Debug, Clone)]
pub struct TasmotaHeaterOutput {
    base_url: String,
    heater_on: bool,
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
            heater_on: false,
        }
    }

    pub fn heater_on(&self) -> bool {
        self.heater_on
    }

    pub fn apply_decision(&mut self, heater_on: bool, reason: &str) -> Result<()> {
        if heater_on == self.heater_on() {
            return Ok(());
        }
        self.set_heater_fail_safe(heater_on, reason)
    }

    pub fn set_heater(&mut self, on: bool, reason: &str) -> Result<()> {
        let url = power_command_url(&self.base_url, on);
        let command_label = if on { "on" } else { "off" };

        info!("sending Tasmota heater {command_label} command: reason={reason}");

        let connection = EspHttpConnection::new(&HttpConfiguration::default())
            .context("failed to create ESP HTTP connection")?;
        let mut client = Client::wrap(connection);
        let request = client
            .get(&url)
            .context("failed to create Tasmota request")?;
        let response = request.submit().context("failed to send Tasmota request")?;

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

    fn set_heater_fail_safe(&mut self, on: bool, reason: &str) -> Result<()> {
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
}
