# ESP32 Temperature Bridge

Firmware for the ESP32-S3-DevKitC-1.

Reads one DS18B20 waterproof probe on GPIO4 and prints labelled temperature readings over USB serial. This is the current single-probe smoke test firmware.

## Protocol

```text
temp,box_air,22.437
```

## Pins

```text
GPIO4 -> box_air DATA
3V3   -> probe adapter VCC
G     -> probe adapter GND / BLK
```

## Setup

Install the ESP Rust tools:

```bash
cargo install espup
espup install
cargo install espflash
```

Load the ESP environment:

```bash
. ~/export-esp.sh
```

## Flash

From this directory:

```bash
ESPFLASH_PORT=/dev/cu.usbmodem1234561 cargo run --release
```

After flashing, the host-side smoke test should see CSV:

```bash
cargo run --manifest-path ../../Cargo.toml -- thermometer-test /dev/cu.usbmodem1234561
```

Expected host output:

```text
time_s,box_air_temp_c,tempeh_core_temp_c
1,22.437,
```

The empty `tempeh_core_temp_c` column is expected until a second probe is added.
