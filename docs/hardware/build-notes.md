# Hardware v0 Build Notes

## Probe wiring: current smoke test

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

Connect one adapter to the ESP32:

```text
adapter VCC        -> ESP32 3V3
adapter GND / BLK  -> ESP32 GND
adapter DATA       -> ESP32 GPIO4
```

This probe is box_air.

Expected serial output from the ESP32:

```text
temp,box_air,22.437
```

Flash the ESP32 firmware:

```bash
cd firmware/esp32-temperature-bridge
ESPFLASH_PORT=/dev/cu.usbmodem1234561 cargo run --release
```

Two-probe wiring will come later. For now, only connect the box_air probe.

## Physical stack

1. Put the seedling heat mat outside, under the SAMLA box.
2. Put the aluminium tray/sheet inside the bottom of the SAMLA.
3. Put the rack above the heat spreader.
4. Place perforated ISTAD bags on the rack.
5. Place the box_air probe in air at rack height, not touching metal/plastic.
6. Leave the lid slightly open, using the probe cable as part of the small air gap.

## Probe naming

- `box_air`: air temperature at rack/food height.
- `product`: not wired yet.

## First test protocol

1. Run the box_air probe at room temperature for 10 minutes.
2. Run empty-box heat test to 30 °C.
3. Run dummy-load test with wet beans/water mass.
4. Only then run food fermentation.
