import importlib.util
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).parents[1] / "scripts" / "mqtt_local.py"
SPEC = importlib.util.spec_from_file_location("mqtt_local_broker_test", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
mqtt_local = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = mqtt_local
SPEC.loader.exec_module(mqtt_local)

PORT = 18_883
TOPIC = "tempeh/tempeh_controller/state"
USERNAME = "tempeh-controller"
PASSWORD = "test-password"


def local_lan_ip() -> str | None:
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as connection:
        try:
            connection.connect(("192.0.2.1", 80))
            return mqtt_local.validate_lan_ip(connection.getsockname()[0])
        except (OSError, ValueError):
            return None


@unittest.skipUnless(
    all(shutil.which(tool) for tool in ("mosquitto", "mosquitto_passwd", "mosquitto_pub", "mosquitto_sub")),
    "Mosquitto broker and client tools are required",
)
class MqttBrokerTests(unittest.TestCase):
    def setUp(self):
        self.lan_ip = local_lan_ip()
        if self.lan_ip is None:
            self.skipTest("a private LAN IPv4 address is required")
        self.directory = tempfile.TemporaryDirectory()
        self.paths = mqtt_local.broker_paths(Path(self.directory.name) / "mqtt")
        self.paths.root.mkdir(mode=0o700)
        self.paths.data.mkdir(mode=0o700)
        mqtt_local.write_config(self.paths, self.lan_ip, PORT)
        subprocess.run(
            ["mosquitto_passwd", "-c", str(self.paths.password), USERNAME],
            input=f"{PASSWORD}\n{PASSWORD}\n",
            text=True,
            check=True,
        )
        self.broker: subprocess.Popen[str] | None = None

    def tearDown(self):
        self.stop_broker()
        self.directory.cleanup()

    def start_broker(self):
        self.broker = subprocess.Popen(
            ["mosquitto", "-c", self.paths.config.name],
            cwd=self.paths.root,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        for _ in range(30):
            if self.broker.poll() is not None:
                output = self.broker.stdout.read() if self.broker.stdout else ""
                self.fail(f"Mosquitto stopped during start: {output}")
            result = subprocess.run(
                [
                    "mosquitto_pub",
                    "-h",
                    self.lan_ip,
                    "-p",
                    str(PORT),
                    "-u",
                    USERNAME,
                    "-P",
                    PASSWORD,
                    "-t",
                    "tempeh/test/ready",
                    "-n",
                ],
                capture_output=True,
                text=True,
            )
            if result.returncode == 0:
                return
            time.sleep(0.1)
        self.fail("Mosquitto did not accept authenticated connections")

    def stop_broker(self):
        if self.broker is None:
            return
        self.broker.terminate()
        try:
            self.broker.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.broker.kill()
            self.broker.wait(timeout=3)
        self.broker = None

    def client(self, *arguments: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(arguments, capture_output=True, text=True)

    def test_authenticated_retained_messages_survive_restart_and_anonymous_access_fails(self):
        self.start_broker()
        subscriber = subprocess.Popen(
            [
                "mosquitto_sub",
                "-h",
                self.lan_ip,
                "-p",
                str(PORT),
                "-u",
                USERNAME,
                "-P",
                PASSWORD,
                "-t",
                TOPIC,
                "-C",
                "1",
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            published = self.client(
                "mosquitto_pub",
                "-h",
                self.lan_ip,
                "-p",
                str(PORT),
                "-u",
                USERNAME,
                "-P",
                PASSWORD,
                "-t",
                TOPIC,
                "-m",
                '{"run_state":"idle"}',
                "-r",
            )
            self.assertEqual(published.returncode, 0, published.stderr)
            received, errors = subscriber.communicate(timeout=5)
            self.assertEqual(subscriber.returncode, 0, errors)
            self.assertEqual(received.strip(), '{"run_state":"idle"}')
        finally:
            if subscriber.poll() is None:
                subscriber.terminate()
                subscriber.wait(timeout=3)

        anonymous = self.client(
            "mosquitto_sub",
            "-h",
            self.lan_ip,
            "-p",
            str(PORT),
            "-t",
            TOPIC,
            "-C",
            "1",
            "-W",
            "1",
        )
        self.assertNotEqual(anonymous.returncode, 0)

        self.stop_broker()
        self.start_broker()
        retained = self.client(
            "mosquitto_sub",
            "-h",
            self.lan_ip,
            "-p",
            str(PORT),
            "-u",
            USERNAME,
            "-P",
            PASSWORD,
            "-t",
            TOPIC,
            "-C",
            "1",
            "-W",
            "3",
        )
        self.assertEqual(retained.returncode, 0, retained.stderr)
        self.assertEqual(retained.stdout.strip(), '{"run_state":"idle"}')


if __name__ == "__main__":
    unittest.main()
