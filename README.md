# Tempeh OS

A project for modelling and eventually controlling a low-cost tempeh incubator.

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

## Test

```bash
cargo test
```
