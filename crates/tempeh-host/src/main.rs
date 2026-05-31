mod cli;
mod csv_log;
mod live_ui;
mod ports;
mod tasmota;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    cli::run()
}
