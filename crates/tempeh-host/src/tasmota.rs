use std::env;
use std::thread;
use std::time::Duration;

use tempeh_control::{ControlLoop, ControlReading, Heater, HeaterError, TraceThermometer};
use tempeh_sim::{SimConfig, TemperatureTrace};

pub(crate) fn tasmota_base_url(url_arg: Option<String>) -> Result<String, std::io::Error> {
    url_arg
        .or_else(|| env::var("TEMPEH_TASMOTA_URL").ok())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "provide a Tasmota URL, e.g. cargo run -p tempeh-host -- plug-test http://192.168.1.50",
            )
        })
}

#[derive(Debug, Clone)]
pub(crate) struct TasmotaHeater {
    base_url: String,
    heater_on: bool,
}

impl TasmotaHeater {
    pub(crate) fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: normalise_base_url(base_url.into()),
            heater_on: false,
        }
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    fn command_url(&self, on: bool) -> String {
        let command = if on { "Power%20On" } else { "Power%20Off" };
        format!("{}/cm?cmnd={command}", self.base_url)
    }
}

impl Heater for TasmotaHeater {
    fn set_heater(&mut self, on: bool) -> Result<(), HeaterError> {
        let url = self.command_url(on);
        ureq::get(&url)
            .call()
            .map_err(|_| HeaterError::CommandFailed)?;
        self.heater_on = on;
        Ok(())
    }

    fn heater_on(&self) -> bool {
        self.heater_on
    }
}

fn normalise_base_url(mut base_url: String) -> String {
    base_url = base_url.trim().trim_end_matches('/').to_string();
    if base_url.starts_with("http://") || base_url.starts_with("https://") {
        base_url
    } else {
        format!("http://{base_url}")
    }
}

pub(crate) fn run_plug_test(url_arg: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let base_url = tasmota_base_url(url_arg)?;
    let mut heater = TasmotaHeater::new(base_url);
    eprintln!("Turning plug on");
    heater
        .set_heater(true)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    thread::sleep(Duration::from_secs(2));
    eprintln!("Turning plug off");
    heater
        .set_heater(false)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    eprintln!("Plug test complete at {}", heater.base_url());
    Ok(())
}

pub(crate) fn run_trace_control_test(
    url_arg: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let base_url = tasmota_base_url(url_arg)?;
    let config = SimConfig::default();
    let trace = TemperatureTrace::new(vec![
        20.0,
        21.0,
        config.controller.target_box_air_temp_c + config.controller.hysteresis_c + 1.0,
        config.controller.target_box_air_temp_c,
    ]);
    let thermometer = TraceThermometer::new(trace);
    let heater = TasmotaHeater::new(base_url);
    let mut control = ControlLoop::new(config.controller, thermometer, heater);
    eprintln!(
        "Sending initial off command to {}",
        control.heater().base_url()
    );
    control
        .heater_mut()
        .set_heater(false)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    println!("{}", ControlReading::csv_header());
    let result = run_trace_control_test_steps(&mut control, config.dt_s);
    let shutdown_result = control
        .heater_mut()
        .set_heater(false)
        .map_err(|error| std::io::Error::other(format!("{error:?}")));
    result?;
    shutdown_result?;
    eprintln!(
        "Trace control test complete. Final command sent: off. Tested plug at {}",
        control.heater().base_url()
    );
    Ok(())
}

fn run_trace_control_test_steps(
    control: &mut ControlLoop<TraceThermometer, TasmotaHeater>,
    dt_s: f32,
) -> Result<(), Box<dyn std::error::Error>> {
    for step in 0..4 {
        let time_s = step as f32 * dt_s;
        let reading = control
            .step(time_s)
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        println!("{}", reading.csv_row());
        thread::sleep(Duration::from_secs(2));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasmota_heater_normalises_base_url() {
        let heater = TasmotaHeater::new("192.168.1.50/");
        assert_eq!(heater.base_url(), "http://192.168.1.50");
    }

    #[test]
    fn tasmota_heater_preserves_explicit_scheme() {
        let heater = TasmotaHeater::new("https://plug.local/");
        assert_eq!(heater.base_url(), "https://plug.local");
    }
}
