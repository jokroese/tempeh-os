use std::env;
use std::io::{self, BufRead, BufReader};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use tempeh_model::TemperatureReading;
use tempeh_protocol::parse_temperature_line;
use tempeh_runtime::{LatestTemperatureReadings, RealRunSample};

use crate::csv_log::CsvLog;
use crate::live_ui::{LiveAppState, SharedLiveAppState, spawn_live_server};
use crate::ports::list_serial_ports;
use crate::serial_capture::SerialCapture;

const DEFAULT_SERIAL_BAUD: u32 = 115_200;
const DEFAULT_LIVE_ADDR: &str = "127.0.0.1:8787";

pub(crate) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let command = env::args().nth(1).unwrap_or_else(|| "help".to_string());

    match command.as_str() {
        "thermometer-test" => run_thermometer_test(env::args().nth(2))?,
        "monitor" | "monitor-live" => {
            run_monitor_live(env::args().nth(2), env::args().nth(3))?;
        }
        "ports" | "list-ports" => {
            list_serial_ports(env::args().any(|arg| arg == "--all"))?;
        }
        "help" | "--help" | "-h" => print_help(),
        other => {
            eprintln!("Unknown command: {other}");
            print_help();
            std::process::exit(2);
        }
    }

    Ok(())
}

fn print_help() {
    eprintln!(
        "Tempeh OS host tools\n\nThe ESP32 firmware controls the heater. These host tools only observe it.\n\nCommands:\n  cargo run -p tempeh-host -- ports                                  # recommend likely ESP32 serial port\n  cargo run -p tempeh-host -- ports --all                            # list all available serial ports\n  cargo run -p tempeh-host -- thermometer-test <port|->              # read temperatures only\n  cargo run -p tempeh-host -- monitor <port|-> [csv]                 # faults, temperatures and serial evidence\n\nSee docs/getting-started.md for the supported user journey.\n\nShortcut:\n  just monitor <port> [csv]"
    );
}

fn run_thermometer_test(source_arg: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let source = source_arg.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "provide a serial port or '-' for stdin, e.g. cargo run -p tempeh-host -- thermometer-test /dev/ttyUSB0",
        )
    })?;
    if source == "-" {
        let stdin = io::stdin();
        let reader = stdin.lock();
        return read_temperature_lines(reader);
    }
    let port = serialport::new(&source, DEFAULT_SERIAL_BAUD)
        .timeout(Duration::from_millis(2_000))
        .open()
        .map_err(|error| {
            std::io::Error::other(format!(
                "failed to open serial port {source} at {DEFAULT_SERIAL_BAUD} baud: {error}"
            ))
        })?;

    eprintln!("Reading temperatures from {source} at {DEFAULT_SERIAL_BAUD} baud.");
    eprintln!("Expected lines from current firmware: temp,box_air,22.437 and temp,room_air,20.125");
    eprintln!("If you see ESP-IDF example logs instead, flash crates/tempeh-firmware-esp32 first.");
    eprintln!("Press Ctrl-C to stop monitoring; the ESP32 continues controlling heat.");
    read_temperature_lines(BufReader::new(port))
}

fn read_temperature_lines<R>(mut reader: R) -> Result<(), Box<dyn std::error::Error>>
where
    R: BufRead,
{
    let start = Instant::now();
    let mut latest = LatestTemperatureReadings::new();
    let mut line = String::new();
    let mut printed_header = false;
    loop {
        line.clear();
        let bytes = match reader.read_line(&mut line) {
            Ok(bytes) => bytes,
            Err(error)
                if error.kind() == io::ErrorKind::TimedOut
                    || error.kind() == io::ErrorKind::WouldBlock =>
            {
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        if bytes == 0 {
            break;
        }
        let parsed = match parse_temperature_line(&line) {
            Ok(Some(parsed)) => parsed,
            Ok(None) => continue,
            Err(error) => {
                eprintln!(
                    "Ignoring invalid temperature line {:?}: {error:?}",
                    line.trim()
                );
                continue;
            }
        };
        latest.update(parsed.probe, parsed.temp_c);
        let time_s = start.elapsed().as_secs_f32();
        let Some(reading) = latest.reading(time_s) else {
            continue;
        };
        if !printed_header {
            println!("{}", TemperatureReading::csv_header());
            printed_header = true;
        }
        println!("{}", reading.csv_row());
    }
    if !printed_header {
        eprintln!(
            "No complete temperature reading received. Need at least a temp,box_air,<°C> line."
        );
    }
    Ok(())
}

fn run_monitor_live(
    source_arg: Option<String>,
    csv_arg: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let source = source_arg.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "provide an ESP32 serial port, e.g. cargo run -p tempeh-host -- monitor /dev/cu.usbmodem1234561",
        )
    })?;
    let csv_path = csv_arg.unwrap_or_else(default_monitor_csv_path);
    let csv_file = PathBuf::from(&csv_path);
    let capture_file = csv_file.with_extension("serial.jsonl");
    if csv_file.exists() || capture_file.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "monitor CSV or serial capture already exists",
        )
        .into());
    }
    let addr: SocketAddr = DEFAULT_LIVE_ADDR.parse()?;

    let reader: Box<dyn BufRead> = if source == "-" {
        Box::new(BufReader::new(io::stdin()))
    } else {
        let port = serialport::new(&source, DEFAULT_SERIAL_BAUD)
            .timeout(Duration::from_millis(2_000))
            .open()
            .map_err(|error| {
                std::io::Error::other(format!(
                    "failed to open serial port {source} at {DEFAULT_SERIAL_BAUD} baud: {error}"
                ))
            })?;
        Box::new(BufReader::new(port))
    };

    let stop_requested = Arc::new(AtomicBool::new(false));
    {
        let stop_requested = Arc::clone(&stop_requested);
        ctrlc::set_handler(move || {
            stop_requested.store(true, Ordering::SeqCst);
        })
        .map_err(|error| {
            std::io::Error::other(format!("failed to install Ctrl-C handler: {error}"))
        })?;
    }

    let header = RealRunSample::csv_header();
    let mut capture = SerialCapture::create(&csv_file)?;
    let mut csv_log = CsvLog::create(&csv_path, header)?;
    let live_state = Arc::new(LiveAppState::new(csv_path.clone()));
    live_state.set_capture_path(capture.path().display().to_string());
    let server_handle = spawn_live_server(Arc::clone(&live_state), addr);

    eprintln!("Starting live monitor.");
    eprintln!("Reading control output from {source} at {DEFAULT_SERIAL_BAUD} baud.");
    eprintln!("Saving data to {csv_path}.");
    eprintln!(
        "Saving complete serial evidence to {}.",
        capture.path().display()
    );
    eprintln!("Live UI: http://{addr}");
    eprintln!("This host monitor is read-only; heater control remains on the ESP32.");
    eprintln!("Press Ctrl-C to stop monitoring; the ESP32 continues controlling heat.");

    // Give the server thread a chance to fail fast on bind errors.
    thread::sleep(Duration::from_millis(100));
    if server_handle.is_finished() {
        match server_handle.join() {
            Ok(Err(error)) => return Err(std::io::Error::other(error).into()),
            Ok(Ok(())) => {
                return Err(std::io::Error::other("live UI server stopped unexpectedly").into());
            }
            Err(_) => {
                return Err(std::io::Error::other("live UI server thread panicked").into());
            }
        }
    }

    run_monitor_live_loop(
        reader,
        Arc::clone(&stop_requested),
        header,
        &mut csv_log,
        Arc::clone(&live_state),
        &mut capture,
    )?;

    if !stop_requested.load(Ordering::SeqCst) {
        eprintln!("Serial connection closed; live status remains available until Ctrl-C.");
        while !stop_requested.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(200));
        }
    }

    eprintln!("Live monitor stopped.");
    Ok(())
}

fn run_monitor_live_loop<R>(
    mut reader: R,
    stop_requested: Arc<AtomicBool>,
    header: &str,
    csv_log: &mut CsvLog,
    live_state: SharedLiveAppState,
    capture: &mut SerialCapture,
) -> Result<(), Box<dyn std::error::Error>>
where
    R: BufRead,
{
    let mut line = String::new();
    let mut printed_header = false;
    let mut capture_ok = true;
    let mut printed_fault: Option<String> = None;

    while !stop_requested.load(Ordering::SeqCst) {
        line.clear();

        let bytes = match reader.read_line(&mut line) {
            Ok(bytes) => bytes,
            Err(error)
                if error.kind() == io::ErrorKind::TimedOut
                    || error.kind() == io::ErrorKind::WouldBlock =>
            {
                continue;
            }
            Err(error) => {
                let now_s = live_state.now_s();
                record_capture_event(
                    capture,
                    &live_state,
                    &mut capture_ok,
                    now_s,
                    "connection",
                    &error.to_string(),
                );
                live_state.serial_disconnected(now_s);
                eprintln!("ESP32 serial connection failed: {error}");
                break;
            }
        };

        if bytes == 0 {
            let now_s = live_state.now_s();
            record_capture_event(
                capture,
                &live_state,
                &mut capture_ok,
                now_s,
                "connection",
                "serial connection closed",
            );
            live_state.serial_disconnected(now_s);
            break;
        }

        let now_s = live_state.now_s();
        if capture_ok {
            if let Err(error) = capture.line(now_s, &line) {
                capture_ok = false;
                eprintln!("Serial evidence recording failed: {error}");
                live_state.capture_failed(error.to_string());
            }
        }
        if line.starts_with("state,")
            || line.starts_with("actuator,")
            || (!line.starts_with("status,")
                && (line.contains("WARN") || line.contains("ERROR") || line.contains("failed")))
        {
            eprintln!("{}", line.trim_end());
        }
        let parsed = live_state.ingest_serial(&line, now_s);
        if line.starts_with("state,") {
            printed_fault = live_state.monitor_snapshot(now_s).fault_reason;
        } else if line.starts_with("status,") && parsed.is_ok() {
            let fault = live_state.monitor_snapshot(now_s).fault_reason;
            if fault != printed_fault {
                if let Some(reason) = fault.as_deref() {
                    eprintln!("Controller fault: {reason}");
                }
                printed_fault = fault;
            }
        }
        let control = match parsed {
            Ok(Some(control)) => control,
            Ok(None) => continue,
            Err(error) => {
                let message = format!("Invalid serial record {:?}: {error}", line.trim());
                eprintln!("{message}");
                live_state.monitor_event(now_s, "parse", &message);
                record_capture_event(
                    capture,
                    &live_state,
                    &mut capture_ok,
                    now_s,
                    "parse",
                    &message,
                );
                continue;
            }
        };

        if !printed_header {
            println!("{header}");
            printed_header = true;
        }

        let sample = RealRunSample {
            time_s: control.time_s,
            room_air_temp_c: control.room_air_temp_c,
            box_air_temp_c: control.box_air_temp_c,
            product_temp_c: control.product_temp_c,
            heater_on: control.heater_on,
            reason: control.reason,
        };

        let row = sample.csv_row();
        println!("{row}");
        csv_log.write_row(&row)?;

        live_state.push_sample(
            sample.time_s,
            sample.room_air_temp_c,
            sample.box_air_temp_c,
            sample.product_temp_c,
            sample.heater_on,
            sample.reason.clone(),
        );
    }

    if !printed_header {
        eprintln!("No control samples received.");
        eprintln!("Expected firmware lines like: control,1,,22.437,23.125,1,below_target");
    }

    Ok(())
}

fn record_capture_event(
    capture: &mut SerialCapture,
    live_state: &LiveAppState,
    capture_ok: &mut bool,
    now_s: f32,
    kind: &str,
    message: &str,
) {
    if *capture_ok {
        if let Err(error) = capture.event(now_s, kind, message) {
            *capture_ok = false;
            eprintln!("Serial evidence recording failed: {error}");
            live_state.capture_failed(error.to_string());
        }
    }
}

fn default_monitor_csv_path() -> String {
    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    format!("out/monitor-{timestamp}.csv")
}
