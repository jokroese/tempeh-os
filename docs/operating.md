# Operating an Established Incubator

Use this page after the [first-build checks](validation.md). The ESP32 controls
the heater locally; a laptop is optional monitoring, not a required part of a
batch.

## Before loading a batch

1. Inspect the box, heat mat, cables and low-voltage probe wiring. Keep all
   mains equipment and joins outside the humid chamber.
2. Power the controller and wait for the LED to become blue. Confirm the plug
   is off. If monitoring is open, glance at the box-air reading to confirm it is
   plausible.
3. Put the prepared bags on the rack and position any optional probes.

## Start, observe and stop

| Controller state | Meaning | Your action |
| --- | --- | --- |
| Amber | Starting or retrying a heater reply | Check the dashboard if it persists. |
| Blue | Idle and ready | Hold **BOOT** for two seconds to start. |
| Green | Running | Inspect the batch according to the food procedure. Press **BOOT** once to stop. |
| Purple | Paused: plug communication cannot be confirmed; no heat is requested | Check the plug and network. The controller resumes automatically once safe control and fresh temperatures are confirmed. Press **BOOT** once to cancel the run. |
| Red | Fault; heat is not requested | Treat the run as stopped and use the fault procedure below. |

The controller targets 30 °C box air and cuts off at 34 °C for box air. An
enabled product probe has its own 34 °C cutoff. Fermenting tempeh produces its
own heat, so an optional product probe and ventilation remain useful when the
heater is off.

When the food procedure says the batch is ready, press **BOOT** once. Confirm
blue idle and the plug off before removing the product. The controller has no
automatic finish timer.

## Faults and power loss

For a red LED, do not restart immediately:

1. Verify that the plug is off. The dashboard shows the last Tasmota command
   confirmation; it is not a live measurement of the relay.
2. If monitoring is running, note the fault reason before resetting the ESP32.
3. Correct the reported cause: probe and cable, high temperature, or unsafe plug control.
4. Hold **BOOT** for two seconds to acknowledge a recovered fault. The LED must
   return to blue idle.
5. Decide, using appropriate food-safety guidance, whether the interrupted food
   batch must be discarded. Hold for two seconds again only to deliberately
   restart heating.

After power loss, the controller boots idle and explicitly configures the plug
off before a run can start. It never resumes a run automatically. If the ESP32,
Wi-Fi or control loop stops renewing heat, the plug’s 20-second lease expires
and it turns off. A communication pause can recover automatically after the plug
and temperature checks pass, regardless of how long communication was absent;
assess any interrupted food batch using the food procedure.

## Troubleshooting

| Symptom | Check | Next action |
| --- | --- | --- |
| No serial port | Use a USB **data** cable and reconnect. On Linux, add your user to the `dialout` group, sign out and back in. | Run `cargo run -p tempeh-host -- ports --all`. |
| Port is busy | Close the flashing monitor or another serial application. | Run the monitor again. Stopping the monitor never stops heating. |
| Blue LED but no readings | Check enabled probe wiring, GPIO assignments and pull-ups. | Keep the heat mat disconnected and repeat the no-load check. |
| Start is rejected or LED becomes red | Inspect the fault reason and recent serial diagnostics. | Correct the reported condition, acknowledge to blue, then deliberately start again. |
| LED stays amber or is not visible | Check USB power and serial boot output. A failed LED driver leaves serial status available. | Do not use LED colour alone; resolve the boot or wiring problem. |
| Plug cannot be reached | Check its fixed local address, Wi-Fi and power. | The controller pauses and requests no heat, then resumes only after confirmed recovery and fresh temperature checks. |

For richer fault details, run the USB monitor and keep its CSV and `.serial.jsonl`
capture: see [monitoring and serial evidence](development.md#monitor-faults-and-keep-serial-evidence).

### What each interruption means

| Observation | Behaviour | Reason |
| --- | --- | --- |
| One ON reply is missing | Amber warning; retry ON after 1 second while the previous confirmed lease still has time. | A timed-out HTTP request may already have switched the plug on. It does not prove the plug failed. |
| No confirmed ON by 15 seconds after the last confirmed renewal | Purple pause; request no heat, retry safe plug configuration every 5 seconds, and resume automatically after confirmed recovery and fresh temperature checks. | There is too little confirmed lease time left to promise continuous heat. The plug's 20-second timer limits any remaining ON period. |
| OFF reply or boot configuration reply is missing | Keep requesting OFF and safe configuration; pause an active run if OFF is unconfirmed. | A missing reply alone does not prove the relay or its safety timer is unsafe. |
| Plug reports the opposite relay state, or rejects `PowerOnState 0` or `PulseTime 120` | Latch red. | The plug has given direct evidence that relay control or the independent safety settings cannot be trusted. |
| Required probe becomes stale, or a hard temperature cutoff is reached | Latch red. | The controller cannot reliably regulate temperature, or heat is already unsafe. An isolated CRC warning does not itself cause a fault; the last valid reading remains usable until it becomes stale. |

If an unanswered ON might arrive after an OFF command, the controller repeats OFF for one full plug lease. A long pause can affect the food batch even when the controller later resumes; assess the batch using the food procedure.
