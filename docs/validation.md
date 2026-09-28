# Practical First-Build Checks

Tempeh OS is a DIY project. These checks are meant to catch wiring, control and
heating problems before a first batch without turning the project into a test
programme.

## What has already worked

- The automated software checks pass locally and run in CI.
- The Nologo ESP32-S3 SuperMini completed a no-load safety check on 9 September
  2026. Probe loss, ESP32 power loss and Wi-Fi loss all resulted in the plug
  turning off.
- The working incubator completed an empty-box warm-up, a dummy-load run and a
  food batch that produced tempeh.

Detailed logs and photographs were not retained for every experiment. There is
no need to recreate them unless they would help answer a current development
question.

## Check a new controller

Keep the heat mat disconnected and follow the
[controller smoke check](../crates/tempeh-firmware-esp32/README.md#controller-smoke-check).
It confirms the parts that matter to the system: safe boot, a plausible box-air
reading, deliberate start and stop, probe failure and expiry of the plug lease
when the controller can no longer renew it.

## Watch the first warm-up

Connect the heat mat and run the finished box empty while you are present.
Confirm that the box approaches 30 °C, the heater cycles off and nothing becomes
unexpectedly hot. Stop and investigate if the temperature approaches the 34 °C
cutoff, the heater does not cycle, or any material or connection becomes too
hot.

No formal log or acceptance threshold is required. Use USB monitoring when a
chart would help you understand unexpected behaviour.

## Make the first batch

Use the [operating guide](operating.md), supervise the first batch and follow
the external food procedure for preparation and assessment. The successful
batch is the useful end-to-end check.

## Repeat only what a change affects

- After control, sensing or plug-lease firmware changes, repeat the controller
  smoke check.
- After changing the board, pins, probe or smart plug, repeat the relevant
  smoke-check steps.
- After changing the heater, box, heat spreader or their placement, watch the
  first empty-box warm-up again.
- Changes to bean mass, bag depth or ventilation do not require repeating the
  system checks; observe the batch normally.

Compare probes side by side or build a dummy load only when investigating a
suspicious reading, probe placement or thermal behaviour.
