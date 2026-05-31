pub(crate) fn list_serial_ports(show_all: bool) -> Result<(), Box<dyn std::error::Error>> {
    let ports = serialport::available_ports()
        .map_err(|error| std::io::Error::other(format!("failed to list serial ports: {error}")))?;

    if ports.is_empty() {
        println!("No serial ports found.");
        return Ok(());
    }

    let candidates = ports
        .iter()
        .filter_map(serial_port_candidate)
        .collect::<Vec<_>>();

    if let Some(best) = best_serial_candidate(&candidates) {
        println!("recommended ESP32 port:");
        println!("  {}", best.port_name);
        println!("    {}", best.reason);
        if best.is_macos_cu {
            println!("    macOS call-out device; preferred for apps opening the port");
        }
        println!("    use:");
        println!(
            "      cargo run -p tempeh-host -- thermometer-test {}",
            best.port_name
        );
        println!();
    } else {
        println!("No obvious ESP32 serial port found.");
        println!(
            "Try unplugging/replugging the ESP32 and running `cargo run -p tempeh-host -- ports --all`."
        );
        println!();
    }

    let other_candidates = candidates
        .iter()
        .filter(|candidate| {
            best_serial_candidate(&candidates)
                .map(|best| best.port_name != candidate.port_name)
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();

    if !other_candidates.is_empty() {
        println!("other likely ESP32-related ports:");
        for candidate in other_candidates {
            println!("  {}", candidate.port_name);
            println!("    {}", candidate.reason);
            if candidate.is_macos_tty {
                println!("    macOS tty variant; usually prefer the matching /dev/cu.* port");
            }
        }
        println!();
    }

    if show_all {
        println!("all serial ports:");
        for port in &ports {
            print_serial_port(port);
        }
    } else {
        println!(
            "Tip: run `cargo run -p tempeh-host -- ports --all` to show Bluetooth/debug/other serial ports too."
        );
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct SerialPortCandidate {
    port_name: String,
    reason: &'static str,
    score: i32,
    is_macos_cu: bool,
    is_macos_tty: bool,
}

fn serial_port_candidate(port: &serialport::SerialPortInfo) -> Option<SerialPortCandidate> {
    let serialport::SerialPortType::UsbPort(info) = &port.port_type else {
        return None;
    };

    let reason = likely_esp32_reason(info)?;
    let is_macos_cu = port.port_name.starts_with("/dev/cu.");
    let is_macos_tty = port.port_name.starts_with("/dev/tty.");

    let mut score = likely_esp32_score(info);

    if is_macos_cu {
        score += 100;
    }

    if is_macos_tty {
        score -= 100;
    }

    Some(SerialPortCandidate {
        port_name: port.port_name.clone(),
        reason,
        score,
        is_macos_cu,
        is_macos_tty,
    })
}

fn best_serial_candidate(candidates: &[SerialPortCandidate]) -> Option<&SerialPortCandidate> {
    candidates.iter().max_by(|a, b| {
        a.score
            .cmp(&b.score)
            .then_with(|| b.port_name.cmp(&a.port_name))
    })
}

fn print_serial_port(port: &serialport::SerialPortInfo) {
    println!("  {}", port.port_name);

    match &port.port_type {
        serialport::SerialPortType::UsbPort(info) => {
            println!("    type: USB");
            println!("    vid: {:04X}", info.vid);
            println!("    pid: {:04X}", info.pid);

            if let Some(serial_number) = info.serial_number.as_deref() {
                println!("    serial: {serial_number}");
            }

            if let Some(manufacturer) = info.manufacturer.as_deref() {
                println!("    manufacturer: {manufacturer}");
            }

            if let Some(product) = info.product.as_deref() {
                println!("    product: {product}");
            }

            if let Some(reason) = likely_esp32_reason(info) {
                println!("    likely: {reason}");
            }
        }
        serialport::SerialPortType::BluetoothPort => {
            println!("    type: Bluetooth");
        }
        serialport::SerialPortType::PciPort => {
            println!("    type: PCI");
        }
        serialport::SerialPortType::Unknown => {
            println!("    type: unknown");
        }
    }
}

fn likely_esp32_reason(info: &serialport::UsbPortInfo) -> Option<&'static str> {
    if info.vid == 0x303A {
        return Some("Espressif native USB device");
    }

    if info.vid == 0x10C4 {
        return Some("Silicon Labs CP210x USB-UART bridge, commonly used on ESP32 dev boards");
    }

    if info.vid == 0x1A86 {
        return Some("WCH CH340/CH910x USB-UART bridge, commonly used on ESP32 dev boards");
    }

    if info.vid == 0x0403 {
        return Some("FTDI USB-UART bridge, sometimes used on ESP32 dev boards");
    }

    let manufacturer = info.manufacturer.as_deref().unwrap_or_default();
    let product = info.product.as_deref().unwrap_or_default();

    if contains_case_insensitive(manufacturer, "espressif")
        || contains_case_insensitive(product, "espressif")
        || contains_case_insensitive(product, "esp32")
    {
        return Some("USB metadata mentions Espressif/ESP32");
    }

    if contains_case_insensitive(manufacturer, "silicon labs")
        || contains_case_insensitive(product, "cp210")
    {
        return Some("Silicon Labs CP210x USB-UART bridge, commonly used on ESP32 dev boards");
    }

    if contains_case_insensitive(manufacturer, "wch")
        || contains_case_insensitive(product, "ch340")
        || contains_case_insensitive(product, "ch910")
    {
        return Some("WCH CH340/CH910x USB-UART bridge, commonly used on ESP32 dev boards");
    }

    if contains_case_insensitive(manufacturer, "ftdi")
        || contains_case_insensitive(product, "ft232")
    {
        return Some("FTDI USB-UART bridge, sometimes used on ESP32 dev boards");
    }

    None
}

fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn likely_esp32_score(info: &serialport::UsbPortInfo) -> i32 {
    if info.vid == 0x303A {
        return 1_000;
    }

    if info.vid == 0x10C4 || info.vid == 0x1A86 || info.vid == 0x0403 {
        return 700;
    }

    let manufacturer = info.manufacturer.as_deref().unwrap_or_default();
    let product = info.product.as_deref().unwrap_or_default();

    if contains_case_insensitive(manufacturer, "espressif")
        || contains_case_insensitive(product, "espressif")
        || contains_case_insensitive(product, "esp32")
    {
        return 900;
    }

    if contains_case_insensitive(manufacturer, "silicon labs")
        || contains_case_insensitive(product, "cp210")
    {
        return 650;
    }

    if contains_case_insensitive(manufacturer, "wch")
        || contains_case_insensitive(product, "ch340")
        || contains_case_insensitive(product, "ch910")
    {
        return 650;
    }

    if contains_case_insensitive(manufacturer, "ftdi")
        || contains_case_insensitive(product, "ft232")
    {
        return 600;
    }

    0
}
