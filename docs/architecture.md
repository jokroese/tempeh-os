# Tempeh OS Architecture

Tempeh OS is a Rust workspace for modelling, observing, and controlling a low-cost tempeh incubator.

The design goal is to keep domain logic shared and hardware adapters explicit:

- pure crates describe vocabulary, control policy, simulation, and text protocols;
- host code owns laptop-side adapters such as serial ports, CSV files, HTTP, and the live UI;
- firmware code owns ESP32-side adapters such as GPIO, DS18B20 probes, and eventually heater output.

## Crate responsibilities

### `tempeh-model`

Owns shared domain vocabulary.

It defines common data types such as:

- `ControllerConfig`
- `EnvironmentState`
- `TemperatureProbe`
- `TemperatureReading`

This crate should stay small and dependency-light. It is the common language used by the rest of the workspace.

### `tempeh-control`

Owns generic control primitives.

It contains the basic hysteresis controller and simple test adapters used for simulated control runs.

It should not know about:

- serial ports;
- Tasmota;
- ESP-IDF;
- GPIO;
- concrete hardware networking.

### `tempeh-runtime`

Owns real-run control policy.

This is the shared safety and decision layer used by both the laptop host and the ESP32 firmware.

It owns:

- latest temperature reading state;
- timestamp and stale-reading logic;
- real-run sample rows;
- hard cutoffs;
- optional product probe semantics;
- mapping latest readings into heater decisions.

Current policy:

- `box_air` is required before a real-run sample can be emitted;
- `room_air` is optional;
- `product` is optional until first seen;
- once `product` has been seen, a stale product reading fails safe;
- stale `box_air` fails safe;
- hard cutoffs fail safe;
- normal hysteresis is driven by `box_air`.

### `tempeh-protocol`

Owns shared text protocols.

At the moment this includes the serial temperature line format:

```text
temp,<probe>,<temperature-c>
```

For example:

```text
temp,box_air,22.437
temp,room_air,20.125
temp,product,23.125
```

Both firmware and host should use this crate rather than hand-rolling protocol strings or parsers.

### `tempeh-sim`

Owns simulation.

The simulator is intentionally approximate. Its job is to support thinking, visualisation, and policy experiments, not to be a calibrated thermal model.

### `tempeh-pet`

Owns the mycelial status report.

This crate translates simulated state into friendly batch status, milestones, readiness estimates, and pet-like messaging.

It is a presentation/domain-narrative crate, not a hardware control crate.

### `tempeh-host`

Owns laptop-side adapters and commands.

This crate contains:

- CLI routing;
- serial-port reading;
- Tasmota HTTP heater adapter;
- CSV logging;
- local live web UI;
- serial port discovery.

The host can currently actuate the Tasmota plug. It does this by reading firmware `temp,...` lines, applying `tempeh-runtime`, and sending HTTP commands to the plug.

### `tempeh-firmware-esp32`

Owns ESP32-side adapters.

This crate currently:

- reads DS18B20 probes;
- emits `temp,...` lines;
- runs `tempeh-runtime::RunSupervisor` on-device;
- accepts deliberate start, stop, and fault-acknowledgement commands from the built-in button;
- emits `control,...`, `state,...`, and `actuator,...` diagnostics;
- renews a time-limited Tasmota heater lease;
- indicates idle, running, and fault states on the built-in RGB LED.

## Current control boundary

The autonomous firmware-evaluated path is the product direction. The older
host-actuated path remains only for controlled engineering experiments.

### Legacy host-actuated experiments

```text
ESP32 probes
  -> temp,... serial lines
  -> tempeh-host
  -> tempeh-runtime
  -> Tasmota HTTP plug command
```

This remains useful for deliberate host-driven experiments. It is not the
recommended incubation path and must not run concurrently with autonomous
actuation.

### Primary autonomous control

```text
ESP32 probes and button
  -> tempeh-runtime::RunSupervisor
  -> renewable Tasmota HTTP lease
```

This path is laptop-independent. The serial records distinguish desired heat
from the relay state confirmed by Tasmota. Optional serial and MQTT consumers
are read-only observers.

## Safety invariants

Any heater-actuating implementation must preserve these invariants:

- the heater starts off on boot;
- cold readings cannot start heating while the supervisor is idle;
- starting and fault acknowledgement require deliberate local input;
- the heater returns off on reset or panic where possible;
- missing `box_air` means no heat;
- stale `box_air` means no heat;
- `product` may be absent for experiments;
- once `product` has been seen, stale `product` means no heat;
- product hard cutoff means no heat;
- box-air hard cutoff means no heat;
- failed actuator commands must be visible in logs;
- heater-on commands must be renewed before the plug-side lease expires;
- Tasmota response bodies must confirm the requested state or setting;
- control decisions and actuator state should be distinguishable in logs.

Firmware runs the policy on a periodic safety tick, not only after successful probe reads. This allows stale-reading safety to turn the heater output off even if probe reads stop producing fresh values.

## Laptop-free operation

The target state is:

```text
ESP32 probes
  -> tempeh-runtime
  -> ESP32 heater adapter
```

The laptop is optional. It remains useful for logs, charts, and debugging, but is not required to keep a supervised run active.

Optional observability follows a separate, one-way path:

```text
RunSupervisor + latest probes + HeaterLease
  -> generic retained MQTT availability and state
  -> optional Home Assistant discovery adapter
  -> Home Assistant Recorder, Node-RED, openHAB, or another consumer
```

MQTT is not part of the control path. Broker failure, Home Assistant failure, and
telemetry backpressure must not change supervisor state or delay lease renewal.
The firmware uses the ESP-IDF MQTT outbox rather than performing broker network
I/O synchronously in the safety loop. The MQTT topic and JSON payload contract lives
in `tempeh-protocol::mqtt`; `tempeh-protocol::home_assistant` only describes that
contract using Home Assistant discovery. Control topics are deliberately out of
scope for this read-only slice.

The selected actuator boundary is:

```text
RunSupervisor -> HeaterLease -> TasmotaHeaterOutput -> Tasmota plug
```

The firmware confirms `PowerOnState 0` and a 20-second `PulseTime` before declaring the actuator ready. It renews the lease every 5 seconds while heat is desired. Failed commands enter a latched fault and stop renewal; Tasmota then turns the relay off when its remaining lease expires. After a network interruption, firmware requests Wi-Fi reconnection and reapplies the safe Tasmota configuration before it permits the fault to be acknowledged.

## Dependency direction

The intended dependency flow is:

```text
tempeh-model
  <- tempeh-control
  <- tempeh-runtime
  <- tempeh-host
  <- tempeh-firmware-esp32

tempeh-model
  <- tempeh-protocol
  <- tempeh-host
  <- tempeh-firmware-esp32
```

Host-only crates such as HTTP clients, serial-port libraries, Axum, and Tokio should not enter the firmware dependency graph.

Firmware-only crates such as ESP-IDF HAL crates should not enter the host path.

The check:

```bash
cargo tree -p tempeh-firmware-esp32 | rg "ureq|rustls|ring"
```

should remain empty.
