# Hardware Build Notes

The canonical user journey is the [complete build and use guide](../getting-started.md).
This page records lower-level details for the current SuperMini wiring and
original prototype.

## Reference board and probes

The current controller is a **Nologo ESP32-S3 SuperMini** with one required probe
and two optional probes:

| Probe | Required | Role | DATA pin |
| --- | --- | --- | --- |
| `box_air` | Yes | Air temperature at food height; normal heater control | GPIO13 |
| `product` | No | Representative bean/bag temperature; independent hard safety cutoff when enabled | GPIO4 |
| `room_air` | No | Ambient context only | GPIO6 |

The built-in BOOT button is on GPIO0 and the tested status LED is on GPIO48.
Other SuperMini revisions need their own verification. The DevKitC-1 is not yet
the tested build: its original revision uses GPIO48 for the LED and revision
1.1 uses GPIO38.

## Original MICREEN probe adapters

The original waterproof DS18B20 kit contained:

- a waterproof probe;
- a screw-terminal adapter module containing the pull-up;
- a three-conductor cable;
- a loose three-legged sensor, which was not used.

For that exact kit, each waterproof probe connected to its adapter as follows:

```text
red     -> VCC
black   -> GND / BLK
yellow  -> DATA
```

Do not assume other manufacturers use the same colours. Verify their data sheet
before applying power.

Connect every enabled adapter VCC to ESP32 3V3 and every adapter GND to ESP32
GND. Connect the DATA line to the GPIO in the reference table.

## Physical arrangement

1. Put the seedling heat mat outside, under the incubation box.
2. Put the aluminium/stainless tray or ceramic heat spreader inside the box.
3. Put the raised rack above the heat spreader.
4. Place perforated food-contact bags on the rack after the first empty-box
   warm-up.
5. Hold `box_air` in free air at rack height without touching metal or plastic.
6. If fitted, hold `product` against the outside of the representative bag
   unless the probe is explicitly rated for food contact.
7. If fitted, keep `room_air` outside the box and away from the mat and draughts.
8. Route probe cables through a small lid gap without crushing them.

## Local controls

| Action | Result |
| --- | --- |
| Power on | Amber while starting, then blue if ready |
| Hold BOOT for two seconds while blue | Start; LED becomes green |
| Press BOOT once while green | Stop; LED becomes blue |
| Fault | LED becomes red and heat is no longer requested |
| Hold BOOT for two seconds after the cause has recovered | Acknowledge; return to blue idle |

A recovered fault never restarts heating automatically. Starting again requires
a second deliberate two-second hold.

## Expected serial records

```text
temp,box_air,22.437
temp,product,23.125
control,1,,22.437,23.125,1,below_target
state,1,running,user_start
actuator,6,1,1,lease_renewed
```

If the optional room probe is enabled, the firmware also emits:

```text
temp,room_air,20.125
```

## First-build checks

1. Complete the controller smoke check with the heat mat disconnected.
2. Watch one empty-box warm-up to the 30 °C target.
3. Supervise the first food batch.

See the [practical first-build checks](../validation.md) for when to repeat a
check. Compare probes or use a dummy load only to investigate a specific
problem.

The no-load firmware check passed on 9 September 2026 using a Nologo ESP32-S3
SuperMini, not the reference ESP32-S3-DevKitC-1. Sensor loss, ESP32 power loss
and Wi-Fi loss each resulted in relay-off behaviour. Two isolated DS18B20 CRC
errors were rejected; inspect the probe connections if errors recur.

The working prototype subsequently completed heated empty-box and dummy-load
runs and successfully made tempeh.
