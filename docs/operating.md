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
| Amber | Starting | Wait for a safe state. |
| Blue | Idle and ready | Hold **BOOT** for two seconds to start. |
| Green | Running | Inspect the batch according to the food procedure. Press **BOOT** once to stop. |
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
3. Correct the cause: probe and cable, high temperature, Wi-Fi, or plug
   connection.
4. Hold **BOOT** for two seconds to acknowledge a recovered fault. The LED must
   return to blue idle.
5. Decide, using appropriate food-safety guidance, whether the interrupted food
   batch must be discarded. Hold for two seconds again only to deliberately
   restart heating.

After power loss, the controller boots idle and explicitly configures the plug
off before a run can start. It never resumes a run automatically. If the ESP32,
Wi-Fi or control loop stops renewing heat, the plug’s 20-second lease expires
and it turns off.

## Troubleshooting

| Symptom | Check | Next action |
| --- | --- | --- |
| No serial port | Use a USB **data** cable and reconnect. On Linux, add your user to the `dialout` group, sign out and back in. | Run `cargo run -p tempeh-host -- ports --all`. |
| Port is busy | Close the flashing monitor or another serial application. | Run the monitor again. Stopping the monitor never stops heating. |
| Blue LED but no readings | Check enabled probe wiring, GPIO assignments and pull-ups. | Keep the heat mat disconnected and repeat the no-load check. |
| Start is rejected or LED becomes red | Inspect the fault reason and recent serial diagnostics. | Correct the reported condition, acknowledge to blue, then deliberately start again. |
| LED stays amber or is not visible | Check USB power and serial boot output. A failed LED driver leaves serial status available. | Do not use LED colour alone; resolve the boot or wiring problem. |
| Plug cannot be reached | Check its fixed local address, Wi-Fi and power. | The controller latches a fault and requests no heat until recovery. |

For richer fault details, run the USB monitor and keep its CSV and `.serial.jsonl`
capture: see [monitoring and serial evidence](development.md#monitor-faults-and-keep-serial-evidence).
