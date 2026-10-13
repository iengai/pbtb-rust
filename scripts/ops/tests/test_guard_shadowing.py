"""A file in scripts/ops named like a standard module runs inside no guard: the hooks, the git helper, the CI agents' gate.

    python -m unittest scripts/ops/tests/test_guard_shadowing.py

scripts/ops is not code-owned as a whole, so a PR can add `scripts/ops/argparse.py`.
guard-shell imports agent_identity from there, and the identity hooks and the git
credential helper run agent_identity.py itself, and the CI agents' gate
(claude_harness.write_settings) runs fix_issue.py or diagnose_issue.py as a
hook; each must reach the standard module, never the file beside it.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SHADOW = "import os\nopen(os.environ['SHADOW_MARKER'], 'w').close()\n"


class ShadowingTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        (self.tmp / ".claude" / "hooks").mkdir(parents=True)
        (self.tmp / "scripts" / "ops").mkdir(parents=True)
        shutil.copy(ROOT / ".claude" / "hooks" / "guard-shell.py", self.tmp / ".claude" / "hooks")
        shutil.copy(ROOT / "scripts" / "ops" / "agent_identity.py", self.tmp / "scripts" / "ops")
        (self.tmp / "scripts" / "ops" / "argparse.py").write_text(SHADOW, encoding="utf-8")
        self.marker = self.tmp / "shadow-ran"
        self.env = {**os.environ, "SHADOW_MARKER": str(self.marker)}

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def run_guard(self, guard: Path, *flags: str) -> None:
        payload = json.dumps({"tool_name": "Bash", "tool_input": {"command": "gh pr list"}})
        subprocess.run([sys.executable, *flags, str(guard)], input=payload, text=True,
                       capture_output=True, env=self.env, timeout=60)

    def test_guard_shell_does_not_import_the_shadow(self):
        self.run_guard(self.tmp / ".claude" / "hooks" / "guard-shell.py", "-P")
        self.assertFalse(self.marker.exists())

    def test_the_shadow_would_run_first_on_the_path(self):
        guard = self.tmp / ".claude" / "hooks" / "guard-shell.py"
        guard.write_text(guard.read_text(encoding="utf-8").replace("sys.path.append(", "sys.path.insert(0, "),
                         encoding="utf-8")
        self.run_guard(guard, "-P")
        self.assertTrue(self.marker.exists())

    def test_agent_identity_with_safe_path_does_not_import_the_shadow(self):
        subprocess.run([sys.executable, "-P", str(self.tmp / "scripts" / "ops" / "agent_identity.py"), "--help"],
                       capture_output=True, env=self.env, timeout=60)
        self.assertFalse(self.marker.exists())

    def test_agent_identity_without_safe_path_would(self):
        subprocess.run([sys.executable, str(self.tmp / "scripts" / "ops" / "agent_identity.py"), "--help"],
                       capture_output=True, env=self.env, timeout=60)
        self.assertTrue(self.marker.exists())

    def test_every_hook_and_the_credential_helper_run_with_safe_path(self):
        settings = json.loads((ROOT / ".claude" / "settings.json").read_text(encoding="utf-8"))
        commands = [h["command"] for groups in settings["hooks"].values() for group in groups for h in group["hooks"]]
        for c in commands:
            if c.startswith("python"):
                self.assertTrue(c.startswith("python -P "), c)
        sys.path.append(str(ROOT / "scripts" / "ops"))
        import agent_identity
        self.assertTrue(agent_identity.CREDENTIAL_HELPER.startswith("!python -P "))

    def test_the_ci_agents_hook_runs_with_safe_path(self):
        sys.path.append(str(ROOT / "scripts" / "ops"))
        import claude_harness
        out = Path(tempfile.mkdtemp())
        try:
            written = claude_harness.write_settings(out / "settings.json", ROOT / "scripts" / "ops" / "fix_issue.py",
                                                    bash=("cargo",), edits=False)
            settings = json.loads(written.read_text(encoding="utf-8"))
        finally:
            shutil.rmtree(out, ignore_errors=True)
        commands = [h["command"] for groups in settings["hooks"].values() for group in groups for h in group["hooks"]]
        self.assertTrue(commands)
        for c in commands:
            self.assertIn(" -P ", c)


if __name__ == "__main__":
    unittest.main()
