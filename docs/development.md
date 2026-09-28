# Development Guide

The current hands-on controller is the Nologo ESP32-S3 SuperMini. The production
direction is autonomous ESP32 control; the laptop host provides simulation,
diagnostics and monitoring. The DevKitC-1 is a later reference target, not the
board currently validated in practice.

## Repository structure

- `tempeh-model` owns shared vocabulary.
- `tempeh-control` owns generic hysteresis control.
- `tempeh-runtime` owns real-run safety, supervision and heater policy.
- `tempeh-protocol` owns serial and MQTT protocols.
- `tempeh-firmware-esp32` owns autonomous probe reading and actuation.
- `tempeh-host` owns simulation, serial diagnostics, logging and the live UI.
- `tempeh-sim` owns the approximate fermentation simulation.
- `tempeh-pet` translates simulated state into experimental narrative status.

See [architecture.md](architecture.md) for dependency boundaries and safety
invariants.

## Host commands

```bash
# Show command help
cargo run -p tempeh-host -- help

# Find a connected ESP32
cargo run -p tempeh-host -- ports

# Monitor autonomous firmware without controlling heat
cargo run -p tempeh-host -- monitor /dev/cu.usbmodem1234561

# Read raw temperature records without controlling heat
cargo run -p tempeh-host -- thermometer-test /dev/cu.usbmodem1234561

# Generate the approximate simulation report
cargo run -p tempeh-host -- html
```

Simulation-only commands are `html`, `csv`, `control` and `pet`. The pet's
progress and readiness estimate are generated from the approximate simulation;
they do not describe a real food batch.

## Monitor faults and keep serial evidence

Run `just monitor <port>` (or `cargo run -p tempeh-host -- monitor <port>`)
and open `http://127.0.0.1:8787`. Monitor mode only observes the autonomous
controller; it does not send plug commands. To replay a serial fixture without
hardware, use `cargo run -p tempeh-host -- monitor - <csv-path>` and pipe the
fixture into stdin.

The browser separates **heat requested** from **plug confirmation**. Confirmation
means a plug command reply, not a live relay measurement. Firmware limits its
reporting validity to one lease duration (normally 20 seconds); an expired or
failed confirmation is **unknown**, even when the last successful reply said ON
or OFF. Temperatures and confirmations show their ages. ESP32 serial activity and
periodic controller status become **stale** after 10 seconds without new input.

Every run writes the existing control CSV and a sibling
`<csv-stem>.serial.jsonl` file containing all serial lines, including warnings
and records that the monitor cannot parse. Both files are flushed as data arrives.
If the LED turns red, check the browser's fault reason and recent diagnostics,
then search the capture for `state,`, `actuator,`, `failed`, and `WARN`. Keep the
capture from before resetting the ESP32; a reset clears its current fault state.

Stopping a host monitor only closes the laptop tool. It never stops an
autonomous controller or cancels a plug lease; use the ESP32 BOOT button to
stop a run.

## Simulation

```bash
cargo run -p tempeh-host -- html
open out/sim.html
cargo run -p tempeh-host -- csv
cargo run -p tempeh-host -- control
cargo run -p tempeh-host -- pet
```

The simulation is intentionally approximate and supports policy exploration; it
is not a calibrated model of food safety or batch readiness.

## Firmware

Follow the installation and configuration stages in
[getting-started.md](getting-started.md). Firmware-specific protocol, MQTT and
build-check details remain in the
[firmware README](../crates/tempeh-firmware-esp32/README.md).

Useful checks:

```bash
just check
just firmware-rebuild
just firmware-config-check
```

## Tests

```bash
cargo test
```
