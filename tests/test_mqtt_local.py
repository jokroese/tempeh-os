import importlib.util
import io
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch


MODULE_PATH = Path(__file__).parents[1] / "scripts" / "mqtt_local.py"
SPEC = importlib.util.spec_from_file_location("mqtt_local", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
mqtt_local = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = mqtt_local
SPEC.loader.exec_module(mqtt_local)


class MqttLocalTests(unittest.TestCase):
    def test_accepts_private_lan_addresses(self):
        self.assertEqual(mqtt_local.validate_lan_ip("192.168.1.25"), "192.168.1.25")
        self.assertEqual(mqtt_local.validate_lan_ip("10.0.0.8"), "10.0.0.8")

    def test_rejects_non_lan_addresses(self):
        for address in ("127.0.0.1", "0.0.0.0", "8.8.8.8", "not-an-ip"):
            with self.assertRaises(ValueError):
                mqtt_local.validate_lan_ip(address)

    def test_setup_creates_private_files_and_preserves_password(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = mqtt_local.broker_paths(Path(directory) / "mqtt")

            def create_password(command, check):
                self.assertEqual(command[1], "-c")
                Path(command[2]).write_text("hashed-password\n")
                return subprocess.CompletedProcess(command, 0)

            with patch.object(mqtt_local, "require_tool", return_value="/tool"), patch.object(
                mqtt_local.subprocess, "run", side_effect=create_password
            ) as run:
                with redirect_stdout(io.StringIO()):
                    self.assertTrue(mqtt_local.setup(paths, "192.168.1.25"))
                    self.assertFalse(mqtt_local.setup(paths, "192.168.1.25"))

            self.assertEqual(run.call_count, 1)
            self.assertIn("allow_anonymous false", paths.config.read_text())
            self.assertIn("listener 1883 192.168.1.25", paths.config.read_text())
            self.assertEqual(paths.password.read_text(), "hashed-password\n")
            self.assertEqual(paths.config.stat().st_mode & 0o777, 0o600)
            self.assertEqual(paths.password.stat().st_mode & 0o777, 0o600)

    def test_refuses_to_replace_non_helper_configuration(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = mqtt_local.broker_paths(Path(directory))
            paths.config.write_text("listener 1883\n")
            with self.assertRaises(RuntimeError):
                mqtt_local.write_config(paths, "192.168.1.25")

    def test_missing_tool_is_reported(self):
        with patch.object(mqtt_local.shutil, "which", return_value=None):
            with self.assertRaisesRegex(RuntimeError, "mosquitto"):
                mqtt_local.require_tool("mosquitto")

    def test_main_reports_invalid_address_and_incomplete_setup(self):
        with redirect_stderr(io.StringIO()):
            self.assertEqual(mqtt_local.main(["setup", "8.8.8.8"]), 1)
        with tempfile.TemporaryDirectory() as directory, redirect_stderr(io.StringIO()):
            self.assertEqual(
                mqtt_local.main(["broker", "--directory", directory]),
                1,
            )


if __name__ == "__main__":
    unittest.main()
