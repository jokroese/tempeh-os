# Watch Your Incubator over Wi-Fi

This route lets a laptop show the ESP32’s read-only telemetry without Home
Assistant. The ESP32 still controls heat locally. USB monitoring remains the
richer diagnostic tool because it shows serial warnings, freshness and a local
capture file.

Use this only on a trusted local network. The local broker requires a password,
but MQTT traffic is plain text: password authentication does not encrypt it.

## Install the viewer and broker

Install [Mosquitto](https://mosquitto.org/download/) and
[MQTT Explorer](https://mqtt-explorer.com/). Python 3 is also required; macOS
and current Ubuntu include it.

On macOS with Homebrew:

```bash
brew install mosquitto
```

On Ubuntu or Debian:

```bash
sudo apt update
sudo apt install mosquitto mosquitto-clients
```

Other Linux distributions need the equivalent packages. Download MQTT Explorer
from its project site rather than assuming a distribution package is current.

## Create the local broker

Choose the laptop’s private LAN IPv4 address. Do not use `127.0.0.1`: the ESP32
cannot reach the laptop through that address. These commands often show the
right address:

```bash
# macOS, usually the Wi-Fi interface
ipconfig getifaddr en0

# Ubuntu or Debian
ip route get 1.1.1.1 | awk '{for (i = 1; i <= NF; i++) if ($i == "src") print $(i + 1)}'
```

From the repository root, create the private project-local broker files and
password. Replace the example with your laptop’s address:

```bash
just mqtt-setup 192.168.1.25
```

The command asks for the `tempeh-controller` password interactively. It stores
the broker configuration, password file and retained messages under ignored
`out/mqtt/`, with private permissions. Running the command again updates the
helper-owned configuration while preserving the existing credentials.

Start the broker in the foreground:

```bash
just mqtt-broker
```

Leave that terminal open during viewing. Press Ctrl-C there to stop the broker.
This workflow neither installs a service nor changes firewall settings. If the
viewer cannot connect, permit local-network access for Mosquitto in the laptop’s
firewall settings.

## Configure and flash the ESP32

Add this to the private
`crates/tempeh-firmware-esp32/firmware.local.toml`, using the same address and
password. `home_assistant_discovery = false` keeps this generic MQTT route
independent of Home Assistant.

```toml
[mqtt]
broker_url = "mqtt://192.168.1.25:1883"
username = "tempeh-controller"
password = "your-mqtt-password"
device_id = "tempeh_controller"
device_name = "Tempeh Controller"
home_assistant_discovery = false
```

Then follow the normal
[firmware flashing step](getting-started.md#stage-4-install-and-flash-the-esp32).
The helper does not edit or flash your firmware configuration.

## Connect MQTT Explorer

Create a connection in MQTT Explorer with:

| Field | Value |
| --- | --- |
| Host | The laptop LAN address, for example `192.168.1.25` |
| Port | `1883` |
| Username | `tempeh-controller` |
| Password | The password chosen during setup |
| Transport | MQTT over plain TCP |

After the ESP32 connects, open the retained topics:

```text
tempeh/tempeh_controller/availability
tempeh/tempeh_controller/state
```

Subscribe to `tempeh/tempeh_controller/event` before a run for fault events,
or before restarting the ESP32 for its boot event. Events are not retained.

`availability` reports the MQTT connection. `state` contains temperatures,
their ages, run and heater state, actuator readiness, fault or pause reason,
retry warning, interruption start time, uptime and
boot ID. An abbreviated example:

```json
{
  "box_air_temp_c": 29.8,
  "box_air_age_s": 1.2,
  "product_temp_c": null,
  "product_age_s": null,
  "run_state": "running",
  "fault_reason": "none",
  "pause_reason": "none",
  "actuator_warning": "none",
  "interruption_started_s": null,
  "desired_heater_on": true,
  "confirmed_heater": "on",
  "last_confirmed_heater": "on",
  "confirmation_age_s": 2.3,
  "lease_duration_s": 20.0,
  "boot_id": "7e4a836c284317a9",
  "uptime_s": 123
}
```

An age is measured when `state` is published; a retained snapshot does not keep
ageing. `null` means no reading or successful heater command yet. Temperatures
can remain visible when stale. `confirmed_heater` becomes `unknown` when its
lease expires; `last_confirmed_heater` preserves the previous reply. A new
`boot_id` means the ESP32 restarted.

Events have `event_type` (`boot`, `fault_raised`, `fault_cleared`), `boot_id`,
`sequence`, `uptime_s` and `fault_reason`. `event_id` combines boot ID and
sequence to identify possible QoS 1 duplicates. Up to 16 unsent events survive
a broker outage in memory; older events are dropped if the buffer fills, and
unsent events are lost on reboot. A fault clears after acknowledgement.

MQTT Explorer does not keep a complete history. Check `availability`, message
receive time and the next live update before trusting a retained snapshot. For
serial warnings and a complete capture, use
[USB monitoring](development.md#monitor-faults-and-keep-serial-evidence).

## Normal interruptions

If the laptop sleeps or the broker stops, the ESP32 keeps controlling heat. It
reconnects and republishes its current state when the broker returns. If the
laptop changes Wi-Fi network or LAN address, run `just mqtt-setup` with the new
address, update `broker_url`, and reflash the ESP32.

When troubleshooting reconnection, keep the heat mat disconnected, stop the
broker and confirm that updates resume after starting it again.

## Optional Home Assistant

If you later use Home Assistant, set `home_assistant_discovery = true` and use
its MQTT integration. The generic topics above remain unchanged. Run the
[optional MQTT check](../crates/tempeh-firmware-esp32/README.md#optional-mqtt-check)
and the [optional Home Assistant check](../crates/tempeh-firmware-esp32/README.md#optional-home-assistant-check)
separately.
