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
- runs `tempeh-runtime::RealRunController` on-device;
- emits diagnostic `control,...` rows;
- applies policy decisions to a dry-run heater output.

It does not actuate the heater yet.

## Current control boundary

There are currently two control paths.

### Host-actuated control

```text
ESP32 probes
  -> temp,... serial lines
  -> tempeh-host
  -> tempeh-runtime
  -> Tasmota HTTP plug command
```

This is the current path for real supervised heat-mat runs.

### Firmware-evaluated control

```text
ESP32 probes
  -> tempeh-runtime
  -> control,... diagnostic rows
```

This proves that the ESP32 can run the same real-run policy as the host.

The firmware path is currently non-actuating. A `heater_on=true` decision in a firmware `control,...` row means “the firmware policy would ask for heat”, not “the firmware has switched heat on”.

## Safety invariants

Any heater-actuating implementation must preserve these invariants:

- the heater starts off on boot;
- the heater returns off on reset or panic where possible;
- missing `box_air` means no heat;
- stale `box_air` means no heat;
- `product` may be absent for experiments;
- once `product` has been seen, stale `product` means no heat;
- product hard cutoff means no heat;
- box-air hard cutoff means no heat;
- failed actuator commands must be visible in logs;
- control decisions and actuator state should be distinguishable in logs.

Firmware runs the policy on a periodic safety tick, not only after successful probe reads. This allows stale-reading safety to turn the heater output off even if probe reads stop producing fresh values.

## Path to laptop-free operation

The target state is:

```text
ESP32 probes
  -> tempeh-runtime
  -> ESP32 heater adapter
```

At that point the laptop is optional. It may still be useful for logs, charts, and debugging, but it should not be required to keep a batch running.

The next actuator design decision is the heater adapter:

1. ESP32 controls a relay or SSR directly.
2. ESP32 controls the existing Tasmota plug over Wi-Fi.

These have different failure modes and should not be mixed casually.

The current firmware-side heater output is dry-run only:

```text
RealRunController -> FirmwareHeaterOutput -> dry-run log
```

The next implementation step is to replace or wrap the dry-run output with exactly one real actuator backend.

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
