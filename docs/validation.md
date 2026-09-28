# Validation Status

This page separates implemented safety behaviour, completed physical testing and
the evidence currently preserved in the repository. A result applies only to
the tested hardware configuration; similar components must not be assumed
equivalent.

Last updated: 9 September 2026.

## Current ladder

| Gate | Tested configuration | Status | Evidence / next action |
| --- | --- | --- | --- |
| Software checks | Host workspace | Passing locally | CI is configured to run formatting, tests, CLI smoke checks and firmware dependency checks |
| Autonomous no-load | ESP32-S3 SuperMini, box-air probe on GPIO13, Tasmota plug, heat mat disconnected | Passed 9 September 2026 | Relay-off behaviour confirmed for probe loss, ESP32 power loss and Wi-Fi loss |
| Probe comparison | Working prototype | Completed | Add exact probes, date, acceptance threshold and result |
| Heated empty box | Working prototype | Completed | Add configuration, temperature log, warm-up, overshoot and cycling summary |
| Heated dummy load | Working prototype | Completed | Add load, probe placement, temperature log and interventions |
| Full food batch | Working prototype and documented food procedure | Completed successfully | Tempeh was made; add batch date, equipment, observations, result and photographs |
| Full ladder repetition | ESP32-S3-DevKitC-1 reference build | Not yet separately recorded | Repeat and document the affected gates on the published reference configuration |

The project author has confirmed that the working setup completed the full cycle
of tests and produced tempeh. The repository currently preserves detailed
written evidence for the 9 September no-load test, while the exact equipment,
measurements and photographs from the later gates still need to be incorporated.
This is an evidence-recording gap, not an assertion that those tests did not
happen.

## Builder checklist

Use this checklist for your own setup. It is separate from the project history
above. Record the board, firmware commit, parts, dates, placement, result and
log location in your own notes. A changed board, probe, heater, box, spreader,
rack, bean mass, bag depth, ventilation or control setting requires the
affected checks again.

### 1. No-load safety check

Keep the heat mat disconnected. Follow the executable
[no-load acceptance check](../crates/tempeh-firmware-esp32/README.md#no-load-acceptance-check)
and confirm all of the following:

- boot reaches blue idle with `state,0,idle,boot_configured` and plug off;
- cold readings do not heat while idle;
- a two-second BOOT hold starts green running and confirmed lease renewals;
- one BOOT press stops the run, returns blue and turns the plug off;
- probe loss, Wi-Fi loss and ESP32 power loss stop renewal so the plug turns off
  no later than 20 seconds after its last renewal;
- a recovered fault remains red until a two-second acknowledgement, returns to
  blue, and needs another deliberate hold before heating can start.

Stop here if any outcome differs. Correct the cause and repeat the whole check.

### 2. Side-by-side probe comparison

Put the two required probes together in stable room air for at least 10 minutes.
Record every reading, the largest difference and any intermittent errors. The
project does not yet publish a measured acceptance threshold for the exact probe
and mounting combination, so do not invent one from the 34 °C safety cutoff.
Investigate drift, disagreement or recurring CRC errors before adding heat.

### 3. Supervised empty-box test

With the final physical stack in place and no food, run under direct
supervision towards the 30 °C target. Record ambient conditions, warm-up,
overshoot, cycling, probe positions and a temperature log. The repository does
not yet preserve a transferable overshoot or timing threshold; compare the run
to the observed behaviour you are prepared to accept before moving on.

### 4. Supervised dummy-load test

Use a representative safe dummy load with the intended bag depth, ventilation,
rack and product-probe placement. Record the relation between box-air and
product readings, temperature trend, interventions and log. Do not proceed to
food until you understand how the external product probe follows the mass.

### 5. First food batch

Use the tested arrangement, start according to the [operating guide](operating.md)
and follow the external food procedure for food preparation and assessment.
Record changes and interruptions. Tempeh OS controls incubation temperature; it
does not determine food safety.

## Gate rule

Complete the gates in order. A failed or materially changed stage invalidates
later assumptions until it has been repeated successfully.

Changes that require relevant tests to be repeated include:

- a different ESP32 board or firmware configuration;
- a different heat mat, plug, box, spreader or rack;
- a different probe model, adapter, GPIO or placement;
- a substantially different product mass, thickness or ventilation arrangement;
- a changed target, hysteresis, cutoff or heater-lease policy.

## Completed working-prototype sequence

The working prototype completed, in order:

1. autonomous no-load safety testing;
2. side-by-side probe comparison;
3. a heated empty-box run;
4. a heated dummy-load run;
5. a complete incubation that produced tempeh.

The food run establishes that the complete system has been used for its intended
purpose. It does not by itself validate every hardware substitution or establish
certification. Add the available photographs and retained run details below so
future builders can reproduce the successful configuration.

## Detailed no-load result

The check passed on a Nologo ESP32-S3 SuperMini with `box_air` on GPIO13 and the
heat mat disconnected:

- boot changed the LED from amber to blue and confirmed the relay off;
- cold readings did not start the idle controller;
- a deliberate hold started the run and confirmed renewable ON commands;
- a press stopped the run, confirmed relay off and returned to blue idle;
- loss of the probe, ESP32 power or Wi-Fi resulted in relay-off behaviour;
- after Wi-Fi recovery, the controller remained fault-latched until deliberate
  acknowledgement, then completed another start/stop cycle.

Two isolated DS18B20 scratchpad CRC errors were safely rejected. Probe
connections must be inspected if errors continue during the comparison or heated
tests.

The executable procedure is maintained in the
[firmware no-load acceptance check](../crates/tempeh-firmware-esp32/README.md#no-load-acceptance-check).

## Local logs

The ignored `out/` directory contains several historical CSV files, including
an empty-box-named file. Their filenames and contents alone do not establish the
complete hardware configuration, date or test decision, so this page does not
claim them as durable evidence for a validation gate. Preserve future logs with
their configuration and a written outcome before relying on them as evidence.

## Evidence to add from the completed runs

- photographs of the complete incubator, wiring, probe positions and finished
  tempeh;
- the exact ESP32, probes, plug, heater, box, spreader and rack used;
- dates and firmware commits for the empty-box, dummy-load and food runs;
- retained temperature logs and a summary of overshoot and heater cycling;
- batch mass, bag thickness, ventilation and product-probe placement;
- any interventions and the criteria used to finish the batch.

## Recording future results

For each physical test, record:

- date, operator and firmware commit;
- exact board, plug, probes, heater and enclosure;
- configuration and probe placement;
- ambient conditions and test duration;
- temperature log and any interventions;
- each acceptance criterion with pass/fail evidence;
- anomalies, photographs and the decision to proceed or repeat.

Store durable test artefacts outside ignored local `out/` files, or add a
sanitised result summary to this page.
