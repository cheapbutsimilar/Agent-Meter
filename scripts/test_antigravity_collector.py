import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

spec = importlib.util.spec_from_file_location(
    "collector", Path(__file__).with_name("antigravity-collector.py"))
collector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(collector)


class CollectorTests(unittest.TestCase):
    def test_environment_excludes_secrets_and_user_customizations(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            env = collector.isolated_environment({
                "SystemRoot": r"C:\Windows", "PATH": "untrusted",
                "USERPROFILE": r"C:\Users\private", "GEMINI_API_KEY": "secret",
                "HTTPS_PROXY": "untrusted", "NODE_OPTIONS": "--require malware",
                "PYTHONPATH": "untrusted", "GOOGLE_CLOUD_PROJECT": "private-project",
            }, root, r"C:\safe\node.exe")
            self.assertEqual(env["USERPROFILE"], str(root / "home"))
            self.assertEqual(env["AGY_CLI_DISABLE_AUTO_UPDATE"], "true")
            self.assertNotIn("secret", json.dumps(env))
            self.assertNotIn("untrusted", json.dumps(env))
            self.assertNotIn("private", json.dumps(env))

    def test_strict_no_tool_permissions_and_only_empty_workspace(self):
        settings = collector.settings_for(Path("empty"), "capture-only")
        self.assertEqual(settings["toolPermission"], "strict")
        self.assertEqual(settings["agentMode"], "")
        self.assertFalse(settings["allowNonWorkspaceAccess"])
        self.assertFalse(settings["useG1Credits"])
        self.assertEqual(settings["permissions"]["allow"], [])
        self.assertIn("mcp(*)", settings["permissions"]["deny"])
        self.assertIn("read_file(*)", settings["permissions"]["deny"])
        self.assertEqual(settings["trustedWorkspaces"], ["empty"])

    def test_old_or_unexpected_capture_is_rejected(self):
        now = time.time() * 1000
        valid = {"version": 1, "capturedAt": now, "quota": {"gemini-weekly": {}}}
        self.assertTrue(collector.valid_snapshot(valid, now - 1))
        self.assertFalse(collector.valid_snapshot(valid, now + 1))
        self.assertFalse(collector.valid_snapshot({**valid, "quota": {"private": {}}}, now - 1))
        self.assertFalse(collector.valid_snapshot({**valid, "quota": {}}, now - 1))

    def test_changed_binary_is_not_executed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fake = root / "agy.exe"
            fake.write_bytes(b"changed")
            self.assertEqual(collector.collect({"agy": str(fake), "agySha256": "wrong"}, root),
                             "cli_changed")

    @unittest.skipUnless(os.name == "nt", "Windows Job Objects")
    def test_job_close_terminates_owned_process(self):
        proc = subprocess.Popen([sys.executable, "-I", "-c", "import time; time.sleep(60)"],
                                creationflags=subprocess.CREATE_NO_WINDOW)
        job = None
        try:
            job = collector.ProcessJob(proc.pid)
            job.close()
            self.assertIsNotNone(proc.wait(timeout=5))
        finally:
            if job:
                job.close()
            if proc.poll() is None:
                proc.kill()
                proc.wait()


if __name__ == "__main__":
    unittest.main()
