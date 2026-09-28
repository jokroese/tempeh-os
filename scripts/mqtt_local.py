#!/usr/bin/env python3
"""Create and run the project-local Mosquitto broker used by Tempeh OS."""

from __future__ import annotations

import argparse
import ipaddress
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path


MANAGED_MARKER = "# Managed by Tempeh OS MQTT helper."
ACCOUNT_NAME = "tempeh-controller"
BROKER_PORT = 1883


@dataclass(frozen=True)
class BrokerPaths:
    root: Path
    config: Path
    password: Path
    data: Path


def broker_paths(root: Path) -> BrokerPaths:
    root = root.resolve()
    return BrokerPaths(root, root / "mosquitto.conf", root / "password", root / "data")


def validate_lan_ip(value: str) -> str:
    try:
        address = ipaddress.IPv4Address(value)
    except ipaddress.AddressValueError as error:
        raise ValueError("use an IPv4 address, such as 192.168.1.25") from error
    if not address.is_private or address.is_loopback or address.is_unspecified:
        raise ValueError("use a private LAN IPv4 address, not loopback or a public address")
    return str(address)


def require_tool(name: str) -> str:
    path = shutil.which(name)
    if path is None:
        raise RuntimeError(f"{name} is not installed or is not on PATH")
    return path


def write_config(paths: BrokerPaths, lan_ip: str, port: int = BROKER_PORT) -> None:
    if paths.config.exists() and MANAGED_MARKER not in paths.config.read_text():
        raise RuntimeError(f"refusing to replace non-helper configuration: {paths.config}")
    content = "\n".join(
        [
            MANAGED_MARKER,
            f"listener {port} {lan_ip}",
            "allow_anonymous false",
            "password_file password",
            "persistence true",
            "persistence_location data/",
            "log_dest stdout",
            "",
        ]
    )
    paths.config.write_text(content)
    paths.config.chmod(0o600)


def setup(paths: BrokerPaths, lan_ip: str) -> bool:
    validate_lan_ip(lan_ip)
    require_tool("mosquitto")
    password_tool = require_tool("mosquitto_passwd")
    paths.root.mkdir(parents=True, exist_ok=True, mode=0o700)
    paths.root.chmod(0o700)
    paths.data.mkdir(exist_ok=True, mode=0o700)
    paths.data.chmod(0o700)
    write_config(paths, lan_ip)

    created_password = False
    if not paths.password.exists():
        print(f"Create the password for MQTT account {ACCOUNT_NAME}.")
        subprocess.run([password_tool, "-c", str(paths.password), ACCOUNT_NAME], check=True)
        paths.password.chmod(0o600)
        created_password = True
    else:
        paths.password.chmod(0o600)

    print(f"Broker configuration: {paths.config}")
    if created_password:
        print("Password file created. Keep it private.")
    else:
        print("Existing password file preserved.")
    print(f"Broker address for the ESP32 and MQTT Explorer: mqtt://{lan_ip}:{BROKER_PORT}")
    print("Use username tempeh-controller and the password entered above.")
    print("Add this to firmware.local.toml, then rebuild and flash:")
    print()
    print("[mqtt]")
    print(f'broker_url = "mqtt://{lan_ip}:{BROKER_PORT}"')
    print('username = "tempeh-controller"')
    print('password = "your-mqtt-password"')
    print('device_id = "tempeh_controller"')
    print('device_name = "Tempeh Controller"')
    print("home_assistant_discovery = false")
    return created_password


def run_broker(paths: BrokerPaths) -> None:
    broker = require_tool("mosquitto")
    if not paths.config.exists() or not paths.password.exists():
        raise RuntimeError("run `just mqtt-setup <laptop-lan-ip>` first")
    if MANAGED_MARKER not in paths.config.read_text():
        raise RuntimeError(f"refusing to run non-helper configuration: {paths.config}")
    print(f"Starting local MQTT broker from {paths.root}. Press Ctrl-C to stop it.")
    subprocess.run([broker, "-c", paths.config.name], cwd=paths.root, check=True)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("setup", "broker"))
    parser.add_argument("lan_ip", nargs="?")
    parser.add_argument("--directory", type=Path, default=Path("out/mqtt"))
    args = parser.parse_args(argv)
    paths = broker_paths(args.directory)
    try:
        if args.command == "setup":
            if args.lan_ip is None:
                parser.error("setup requires the laptop's LAN IPv4 address")
            setup(paths, args.lan_ip)
        else:
            if args.lan_ip is not None:
                parser.error("broker does not take a LAN address")
            run_broker(paths)
    except (RuntimeError, subprocess.CalledProcessError, ValueError) as error:
        print(f"MQTT setup failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
