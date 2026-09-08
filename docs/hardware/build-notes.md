# Hardware v0 Build Notes

## Probe wiring: box + room test

We are using the MICREEN DS18B20 waterproof temperature sensor kit.

Each kit has:

- waterproof probe
- screw-terminal adapter module
- 3-prong cable
- loose three-legged sensor chip

Use the waterproof probe, adapter module, and 3-prong cable. Ignore the loose three-legged chip.

Connect each waterproof probe to its adapter:

```text
red     -> VCC
black   -> GND / BLK
yellow  -> DATA
```

Connect the box-air adapter to the ESP32:

```text
box adapter VCC        -> ESP32 3V3
box adapter GND / BLK  -> ESP32 GND
box adapter DATA       -> ESP32 GPIO13
```

Connect the room-air adapter to the ESP32:

```text
room adapter VCC        -> ESP32 3V3
room adapter GND / BLK  -> ESP32 GND
room adapter DATA       -> ESP32 GPIO6
```

Connect the product adapter to the ESP32:

```text
product adapter VCC        -> ESP32 3V3
product adapter GND / BLK  -> ESP32 GND
product adapter DATA       -> ESP32 GPIO4
```

The controller uses the board hardware for local supervision:

```text
built-in BOOT button       -> GPIO0
built-in addressable LED   -> GPIO48
```

Hold BOOT for 2 seconds to start or acknowledge a recovered fault. Press it once to stop a running controller. LED colours are amber for boot, blue for idle, green for running, and red for fault.

Expected serial output from the ESP32:

```text
temp,box_air,22.437
temp,room_air,20.125
temp,product,23.125
```

Flash the ESP32 firmware:

```bash
cd crates/tempeh-firmware-esp32
ESPFLASH_PORT=/dev/cu.usbmodem1234561 cargo run --release
```

## Physical stack

1. Put the seedling heat mat outside, under the SAMLA box.
2. Put the aluminium tray/sheet inside the bottom of the SAMLA.
3. Put the rack above the heat spreader.
4. Place perforated ISTAD bags on the rack.
5. Place the box_air probe in air at rack height, not touching metal/plastic.
6. Place the room_air probe outside the box, away from the heat mat and direct drafts.
7. Leave the lid slightly open, using the probe cable as part of the small air gap.

## Probe naming

- `box_air`: air temperature at rack/food height, GPIO13, used for normal heater control.
- `room_air`: ambient room temperature outside the incubator, GPIO6, logged only.
- `product`: bean mass / bag-adjacent temperature, GPIO4, logged and used as hard safety cutoff by the host controller.

## First test protocol

1. Disconnect the heat mat and complete the firmware no-load acceptance check.
2. Run the box_air and room_air probes side by side at room temperature for 10 minutes.
3. Run empty-box heat test to 30 °C.
4. Run dummy-load test with wet beans/water mass.
5. Only then run food fermentation.

The no-load firmware check passed on 9 September 2026 with the heat mat disconnected. Sensor loss, ESP32 power loss, and Wi-Fi loss each resulted in relay-off behaviour. Wi-Fi and Tasmota recovered without an ESP32 reboot; the controller remained fault-latched until a deliberate acknowledgement and then completed another supervised start/stop cycle. Two isolated DS18B20 CRC errors were safely rejected; monitor this during the next probe comparison test and inspect the connections if it continues.
