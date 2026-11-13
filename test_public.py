"""What leaves the dispatcher over HTTP: never a session's inbox token.

Until 2026-11-12 /api/sessions and /api/stream returned every dispatchable
session's `token` to whatever asked 127.0.0.1:8792 - any process, another
Windows account's included - and with it the power to put a prompt straight
into that session. Nothing outside the dispatcher used it.

Run: python test_public.py
"""
import json
import threading
import time
import unittest
import urllib.request

from imperatorium import dispatcher
from test_dispatcher import Base

SECRET = "0123456789abcdef0123456789abcdef"


class NoTokens(Base):
    def setUp(self):
        super().setUp()
        self.rows = [{"sessionId": "s1", "key": "s1", "name": "one", "socket": r"\\.\pipe\x",
                      "token": SECRET, "dispatchable": True},
                     {"sessionId": "s2", "key": "s2", "name": "two"}]
        snap = {"at": time.time(), "sessions": self.rows}
        self.control.sessions = lambda max_age=2.0: snap
        dispatcher.Handler.control = self.control
        self.srv = dispatcher.Server(("127.0.0.1", 0), dispatcher.Handler)
        threading.Thread(target=self.srv.serve_forever, daemon=True).start()
        self.port = self.srv.server_address[1]

    def tearDown(self):
        self.srv.shutdown()
        self.srv.server_close()
        super().tearDown()

    def raw(self, path, limit=None):
        with urllib.request.urlopen("http://127.0.0.1:%d%s" % (self.port, path), timeout=5) as fh:
            return fh.read(limit) if limit else fh.read()

    def test_the_session_list_has_no_tokens(self):
        body = self.raw("/api/sessions")
        self.assertNotIn(SECRET.encode(), body)
        rows = json.loads(body)["sessions"]
        self.assertNotIn("token", rows[0])
        self.assertTrue(rows[0]["dispatchable"])   # what the window needs stays
        self.assertEqual(rows[0]["socket"], r"\\.\pipe\x")

    def test_the_stream_has_no_tokens(self):
        url = "http://127.0.0.1:%d/api/stream" % self.port
        with urllib.request.urlopen(url, timeout=5) as fh:
            lines = []
            while not any(l.startswith(b"data:") for l in lines):
                lines.append(fh.readline())
        first = b"".join(lines)
        self.assertIn(b"event: sessions", first)
        self.assertNotIn(SECRET.encode(), first)
        self.assertIn(b'"dispatchable": true', first)

    def test_the_dispatcher_keeps_its_own_copy(self):
        self.raw("/api/sessions")
        self.assertEqual(self.rows[0]["token"], SECRET)

    def test_public_snapshot_copies(self):
        snap = {"at": 1, "sessions": [{"sessionId": "a", "token": "t", "x": 1}]}
        out = dispatcher.public_snapshot(snap)
        self.assertEqual(out["sessions"], [{"sessionId": "a", "x": 1}])
        self.assertEqual(snap["sessions"][0]["token"], "t")


if __name__ == "__main__":
    unittest.main()
