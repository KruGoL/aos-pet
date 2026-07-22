"""Unit tests for the HTTP bridge routing. No sockets, no AOS: BridgeCore is
exercised directly with a fake MCP session."""
import json
import unittest

import pet


def mcp_ok(payload):
    return {"result": {"content": [{"type": "text", "text": json.dumps(payload)}]}}


def mcp_domain_error(text):
    return {"result": {"isError": True,
                       "content": [{"type": "text", "text": json.dumps(text)}]}}


class FakeSession:
    """Scripted session: pops one canned response per call; None = pipe death."""
    def __init__(self, script):
        self.script = list(script)
        self.calls = []
        self.closed = False

    def call(self, tool, args=None):
        self.calls.append((tool, args or {}))
        return self.script.pop(0) if self.script else None

    def close(self):
        self.closed = True


class BridgeCoreTest(unittest.TestCase):
    def core(self, *sessions):
        pool = list(sessions)
        return pet.BridgeCore(lambda: pool.pop(0)), sessions

    def test_health_needs_no_session(self):
        core, _ = self.core()          # factory would explode if called
        self.assertEqual(core.handle("GET", "/health", None), (200, {"ok": True}))

    def test_options_preflight(self):
        core, _ = self.core()
        self.assertEqual(core.handle("OPTIONS", "/action", None), (204, None))

    def test_status_returns_tool_payload(self):
        session = FakeSession([mcp_ok({"name": "Rex", "mood": "happy"})])
        core, _ = self.core(session)
        status, payload = core.handle("GET", "/status", None)
        self.assertEqual(status, 200)
        self.assertEqual(payload["name"], "Rex")
        self.assertEqual(session.calls, [("pet_status", {})])

    def test_action_maps_whitelisted_tool(self):
        session = FakeSession([mcp_ok({"message": "yum"})])
        core, _ = self.core(session)
        status, payload = core.handle("POST", "/action", {"tool": "feed"})
        self.assertEqual(status, 200)
        self.assertEqual(payload["message"], "yum")
        self.assertEqual(session.calls[0][0], "pet_feed")

    def test_action_passes_args(self):
        session = FakeSession([mcp_ok({"message": "hi"})])
        core, _ = self.core(session)
        core.handle("POST", "/action", {"tool": "adopt", "args": {"name": "Bob"}})
        self.assertEqual(session.calls[0], ("pet_adopt", {"name": "Bob"}))

    def test_action_rejects_unknown_tool(self):
        core, _ = self.core()
        status, payload = core.handle("POST", "/action", {"tool": "rm_rf"})
        self.assertEqual(status, 400)
        self.assertIn("error", payload)

    def test_domain_error_is_200(self):
        session = FakeSession([mcp_domain_error("no pet adopted")])
        core, _ = self.core(session)
        status, payload = core.handle("GET", "/status", None)
        self.assertEqual(status, 200)
        self.assertIn("no pet", payload["error"])

    def test_transport_death_reconnects_once_then_503(self):
        dead1, dead2 = FakeSession([]), FakeSession([])
        core, _ = self.core(dead1, dead2)
        status, payload = core.handle("GET", "/status", None)
        self.assertEqual(status, 503)
        self.assertIn("error", payload)
        self.assertTrue(dead1.closed)      # first session dropped before retry

    def test_transport_death_then_recovery(self):
        dead = FakeSession([])
        fresh = FakeSession([mcp_ok({"name": "Rex"})])
        core, _ = self.core(dead, fresh)
        status, payload = core.handle("GET", "/status", None)
        self.assertEqual(status, 200)
        self.assertEqual(payload["name"], "Rex")

    def test_unknown_path_404(self):
        core, _ = self.core()
        self.assertEqual(core.handle("GET", "/nope", None)[0], 404)


if __name__ == "__main__":
    unittest.main()
