# ESP32 Temperature Bridge

Firmware for the ESP32-S3-DevKitC-1.

Reads DS18B20 waterproof probes on separate GPIO pins and prints labelled temperature readings over USB serial.

## Protocol

```text
temp,box_air,22.437
temp,room_air,20.125
temp,product,23.125
```

## Pins

```text
GPIO5 -> box_air DATA
GPIO6 -> room_air DATA
GPIO4 -> product DATA
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

After flashing, run the host-side smoke test from the repository root:

```bash
cargo run -p tempeh-host -- thermometer-test /dev/cu.usbmodem1234561
```

For a supervised heat-mat control run from the repository root:

```bash
cargo run -p tempeh-host -- real-control-test /dev/cu.usbmodem1234561 http://192.168.8.193 out/heat-mat-empty-box-01.csv
```

Expected host output:

```text
time_s,room_air_temp_c,box_air_temp_c,product_temp_c
1,20.125,22.437,23.125
```

The product_temp_c column remains blank until the product probe has emitted at least one valid reading.
