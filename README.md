# Tempeh OS

A project for modelling and eventually controlling a low-cost tempeh incubator.

## Hardware

The physical prototype is documented in `docs/hardware/`. Hardware v0 is a prototype. It uses a SAMLA storage box as the warm-air chamber, while food remains inside perforated food-contact ISTAD bags.

## Crates

- `tempeh-model` owns the vocabulary.
- `tempeh-control` owns the decisions.
- `tempeh-sim` owns the imaginary physics.
- `tempeh-runtime` owns real-run safety and heater decision policy.
- `tempeh-pet` owns the mycelial status report.
- `tempeh-host` owns the laptop-side CLI, serial reader, CSV logging, Tasmota HTTP control, and live UI.
- `tempeh-firmware-esp32` owns ESP32 probe firmware.

## Run

```bash
cargo run -p tempeh-host -- html
open out/sim.html
```

## CSV

```bash
cargo run -p tempeh-host -- csv
```

## Simulated control loop

```bash
cargo run -p tempeh-host -- control
```

## Tasmota plug test

`plug-test` checks that the plug responds: on, wait two seconds, off.

```bash
cargo run -p tempeh-host -- plug-test http://192.168.8.193
```

`trace-control-test` checks the full controller path using fake temperature readings:

```
TemperatureTrace -> TraceThermometer -> Controller -> TasmotaHeater
```

```bash
cargo run -p tempeh-host -- trace-control-test http://192.168.8.193
```

## ESP32 temperature bridge firmware

The ESP32 must be flashed before thermometer-test can read real serial data.

The firmware lives in:

```
crates/tempeh-firmware-esp32
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

Flash and monitor the ESP32-S3 (from the firmware crate directory):

```bash
cd crates/tempeh-firmware-esp32
ESPFLASH_PORT=/dev/cu.usbmodem1234561 cargo run --release
```

The firmware currently reads three DS18B20 probes on separate pins:

```
temp,box_air,22.437
temp,room_air,20.125
temp,product,23.125
```

Probe GPIO mapping: box_air → GPIO5, room_air → GPIO6, product → GPIO4.

## Real control smoke test

`real-control-test` reads the ESP32 temperature bridge and drives the Tasmota plug from the real box-air temperature.

For this control test, `box_air` drives the normal heater decision. `room_air` is logged as ambient context. `product` is logged and used as a hard safety cutoff when available.

It writes the control log to stdout and to a CSV file:

```bash
cargo run -p tempeh-host -- real-control-test /dev/cu.usbmodem1234561 http://192.168.8.193
```

Default output (timestamped so runs do not overwrite each other):

```text
out/real-control-test-20260529-205812.csv
```

Use a named file for a supervised heat-mat run:

```bash
cargo run -p tempeh-host -- real-control-test /dev/cu.usbmodem1234561 http://192.168.8.193 out/heat-mat-empty-box-01.csv
```

## Live real control UI

`real-control-live` runs the same supervised host control loop as `real-control-test`, writes the same CSV log, and serves a local live chart:

```bash
cargo run -p tempeh-host -- real-control-live /dev/cu.usbmodem1234561 http://192.168.8.193
```

Open:

```text
http://127.0.0.1:8787
```

Press Ctrl-C to stop. The command will try to leave the plug off before exiting.

## Real thermometer smoke test

`thermometer-test` reads labelled temperature lines from stdin or a serial port.

Current ESP32 firmware output:

```
temp,box_air,22.437
temp,room_air,20.125
temp,product,23.125
```

Use stdin for parser testing:

```bash
printf "temp,box_air,22.4\ntemp,room_air,20.2\ntemp,product,23.1\n" | cargo run -p tempeh-host -- thermometer-test -
```

Use a serial port for the ESP32 temperature bridge:

```bash
cargo run -p tempeh-host -- thermometer-test /dev/ttyUSB0
cargo run -p tempeh-host -- thermometer-test /dev/ttyACM0
cargo run -p tempeh-host -- thermometer-test /dev/cu.usbmodem1234561
```

Expected CSV output:

```text
time_s,room_air_temp_c,box_air_temp_c,product_temp_c
1,20.125,22.437,23.125
```

Older thermometer logs may use a `tempeh_core_temp_c` column name for the same probe reading.

## Serial ports

List available serial ports to help find the ESP32:

```bash
cargo run -p tempeh-host -- ports
```

The command prints USB metadata where available and marks ports that look like likely ESP32 devices.

The command prints CSV snapshots with the latest known box-air, room-air, and product temperatures. The product column is blank until the product probe has emitted at least one valid reading.
It does not control the heater.

## Pet mode

`pet` turns the latest simulation state into a mycelial status report:

It includes a batch diary that narrates major milestones such as warm-up, metabolic heat, heat risk, and readiness.

```bash
cargo run -p tempeh-host -- pet
```

## Test

```bash
cargo test
```
