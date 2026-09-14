"""The template scripts publish the public catalogue's audience overlay.

    python -m unittest scripts/ops/tests/test_template_audiences.py
"""
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
import backtest_templates  # noqa: E402


class FakeAws:
    """Records each aws command instead of running it."""

    def __init__(self):
        self.calls: list[tuple[list[str], dict]] = []

    def __call__(self, cmd, **kwargs):
        self.calls.append((cmd, kwargs))

    def uploads(self) -> list[dict]:
        return [json.loads(kw["input"]) for cmd, kw in self.calls if cmd[:3] == ["aws", "s3", "cp"]]


def templates(**audiences) -> tempfile.TemporaryDirectory:
    scratch = tempfile.TemporaryDirectory()
    root = Path(scratch.name)
    for name, pbtb in audiences.items():
        (root / f"{name}.json").write_text(json.dumps({"pbtb": pbtb, "bot": {}}), encoding="utf-8")
    # The prefix mirrored as a zero-byte object, as `aws s3 sync` leaves it.
    (root / "predefined.json").write_bytes(b"")
    return scratch


class PublishedTemplatesTest(unittest.TestCase):
    def test_only_an_operator_audience_retires_a_template(self):
        with templates(
            open={"title": "Open"},
            retired={"audience": "operator"},
            other={"audience": "member"},
        ) as root:
            self.assertEqual(backtest_templates.published_templates(Path(root)), ["open", "other"])

    def test_the_overlay_is_uploaded_with_the_public_cache_header(self):
        aws = FakeAws()
        with templates(open={}, retired={"audience": "operator"}) as root:
            backtest_templates.publish_template_audiences(Path(root), "dev", run=aws)
        (cmd, kwargs), = aws.calls
        self.assertIn(backtest_templates.AUDIENCES_URL, cmd)
        self.assertEqual(cmd[cmd.index("--cache-control") + 1], "public, max-age=30")
        self.assertEqual(cmd[cmd.index("--profile") + 1], "dev")
        self.assertEqual(aws.uploads()[0]["published"], ["open"])
        self.assertIsInstance(aws.uploads()[0]["generated_at"], int)

    def test_a_run_on_cached_templates_publishes_nothing(self):
        aws = FakeAws()
        backtest_templates.publish_if_synced(False, "dev", run=aws)
        self.assertEqual(aws.calls, [])

    def test_a_synced_run_publishes_from_a_fresh_sync_not_its_mirror(self):
        aws = FakeAws()
        backtest_templates.publish_if_synced(True, "dev", run=aws)
        (sync, _), (upload, _) = aws.calls
        self.assertEqual(sync[:3], ["aws", "s3", "sync"])
        mirror = str(backtest_templates.DEFAULT_CACHE_DIR)
        self.assertFalse(sync[4].startswith(mirror), f"synced into the run's mirror: {sync[4]}")
        self.assertEqual(upload[:3], ["aws", "s3", "cp"])

    def test_a_script_without_a_mirror_syncs_before_publishing(self):
        aws = FakeAws()
        backtest_templates.republish_template_audiences(None, run=aws)
        self.assertEqual([cmd[:3] for cmd, _ in aws.calls], [["aws", "s3", "sync"], ["aws", "s3", "cp"]])
        self.assertNotIn("--profile", aws.calls[1][0], "no profile: the CLI's own default")


if __name__ == "__main__":
    unittest.main()
