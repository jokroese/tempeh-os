# ESP32 Temperature Bridge

Firmware for the ESP32-S3-DevKitC-1.

Reads DS18B20 waterproof probes on separate GPIO pins and prints labelled temperature readings over USB serial.

## Protocol

```text
temp,box_air,22.437
temp,room_air,20.125
```

## Pins

```text
GPIO4 -> box_air DATA
GPIO5 -> room_air DATA
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

For a supervised heat-mat control run from the repository root:

```bash
cargo run -- real-control-test /dev/cu.usbmodem1234561 http://192.168.8.193 out/heat-mat-empty-box-01.csv
```

Expected host output:

```text
time_s,room_air_temp_c,box_air_temp_c,tempeh_core_temp_c
1,20.125,22.437,
```

The empty `tempeh_core_temp_c` column is expected until the product probe is added.
