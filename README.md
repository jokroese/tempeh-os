# Tempeh OS

A project for modelling and eventually controlling a low-cost tempeh incubator.

## Hardware

The physical prototype is documented in `docs/hardware/`. Hardware v0 is a prototype. It uses a SAMLA storage box as the warm-air chamber, while food remains inside perforated food-contact ISTAD bags.

## Crates

- `tempeh-model` owns the vocabulary.
- `tempeh-control` owns the decisions.
- `tempeh-sim` owns the imaginary physics.
- `tempeh-os`: CLI for composing simulation, control, reports, and experiments.

## Run

```bash
cargo run
open out/sim.html
```

## CSV

```bash
cargo run -- csv
```

## Simulated control loop

```bash
cargo run -- control
```

## Tasmota plug test

`plug-test` checks that the plug responds: on, wait two seconds, off.

```bash
cargo run -- plug-test http://192.168.8.193
```

`trace-control-test` checks the full controller path using fake temperature readings:

```
TemperatureTrace -> TraceThermometer -> Controller -> TasmotaHeater
```

```bash
cargo run -- trace-control-test http://192.168.8.193
```

## ESP32 temperature bridge firmware

The ESP32 must be flashed before thermometer-test can read real serial data.

The firmware lives in:

```
firmware/esp32-temperature-bridge
```

Install the ESP Rust tools:

```bash
cargo install espup
espup install
cargo install espflash
```

Load the ESP toolchain environment in the current shell:

```bash
. ~/export-esp.sh
```

Flash and monitor the ESP32-S3:

```bash
cd firmware/esp32-temperature-bridge
ESPFLASH_PORT=/dev/cu.usbmodem1234561 cargo run --release
```

The firmware currently reads one DS18B20 probe on GPIO4 and labels it as box_air:

```
temp,box_air,22.437
```

## Real thermometer smoke test

`thermometer-test` reads labelled temperature lines from stdin or a serial port.

Current ESP32 firmware output:

```
temp,box_air,22.437
```

Use stdin for parser testing:

```bash
printf "temp,box_air,22.4\n" | cargo run -- thermometer-test -
```

Use a serial port for the ESP32 temperature bridge:

```bash
cargo run -- thermometer-test /dev/ttyUSB0
cargo run -- thermometer-test /dev/ttyACM0
cargo run -- thermometer-test /dev/cu.usbmodem1234561
```

Expected CSV output for the current single-probe firmware:

```text
time_s,box_air_temp_c,tempeh_core_temp_c
1,22.437,
```

## Serial ports

List available serial ports to help find the ESP32:

```bash
cargo run -- ports
```

The command prints USB metadata where available and marks ports that look like likely ESP32 devices.

The command prints CSV snapshots with the latest known box-air temperature. The product/core column is blank until a second probe is added.
It does not control the heater.

## Pet mode

`pet` turns the latest simulation state into a mycelial status report:

It includes a batch diary that narrates major milestones such as warm-up, metabolic heat, heat risk, and readiness.

```bash
cargo run -- pet
```

## Test

```bash
cargo test
```
