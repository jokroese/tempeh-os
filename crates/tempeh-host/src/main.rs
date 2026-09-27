mod cli;
mod csv_log;
mod live_ui;
mod monitor;
mod ports;
mod serial_capture;
mod tasmota;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    cli::run()
}
