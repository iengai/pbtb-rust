"""smoke-lambda sends each function an event its runtime can deserialize.

    python -m unittest scripts/ops/tests/test_smoke_lambda.py
"""
from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import pbtb_ops  # noqa: E402

C = pbtb_ops.cfg("dev")
OK = {"StatusCode": 200}


class SmokePayloadTest(unittest.TestCase):
    def test_mcp_http_gets_an_http_api_v2_request(self):
        self.assertTrue(pbtb_ops.smoke_is_http(C["lambdas"]["mcp-http"], C))
        event = json.loads(pbtb_ops.SMOKE_HTTP_V2)
        self.assertEqual(event["version"], "2.0")
        self.assertEqual(event["requestContext"]["http"]["method"], "POST")
        self.assertNotIn("authorization", {k.lower() for k in event["headers"]})

    def test_event_handlers_get_the_eventbridge_shape(self):
        for short in ("task-state", "daily-pnl"):
            self.assertFalse(pbtb_ops.smoke_is_http(C["lambdas"][short], C))
        event = json.loads(pbtb_ops.SMOKE_EVENTBRIDGE)
        self.assertEqual(event["source"], "pbtb.smoke-test")


class SmokeVerdictTest(unittest.TestCase):
    def test_an_event_handler_passes_on_200_without_function_error(self):
        self.assertIsNone(pbtb_ops.smoke_failure(False, OK, "null"))

    def test_a_function_error_fails(self):
        out = {"StatusCode": 200, "FunctionError": "Unhandled"}
        self.assertIn("Unhandled", pbtb_ops.smoke_failure(False, out, "{}"))

    def test_mcp_http_passes_only_on_a_401_refusal(self):
        self.assertIsNone(pbtb_ops.smoke_failure(True, OK, '{"statusCode":401,"body":""}'))
        self.assertIn("200", pbtb_ops.smoke_failure(True, OK, '{"statusCode":200}'))
        self.assertIsNotNone(pbtb_ops.smoke_failure(True, OK, "null"))
        self.assertIsNotNone(pbtb_ops.smoke_failure(True, OK, "not json"))


if __name__ == "__main__":
    unittest.main()
