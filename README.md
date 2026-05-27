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

## Test

```bash
cargo test
```
