"""The dispatcher's conversation cache: parse once per transcript change."""
import os
import shutil
import tempfile
import threading
import time
import unittest

from ccontrol import convcache, conversation


def item(speaker, text):
    return {"speaker": speaker, "role": "assistant" if speaker != "you" else "user",
            "blocks": [{"kind": "text", "text": text}]}


class CacheTests(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.files = {}
        self.raw = {}
        self.reads = []

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def transcript(self, sid, text="x"):
        path = self.files.setdefault(sid, os.path.join(self.dir, sid + ".jsonl"))
        with open(path, "a", encoding="utf-8") as fh:
            fh.write(text)

    def cache(self, **kw):
        def read(sid):
            self.reads.append(sid)
            return list(self.raw.get(sid, []))
        return convcache.Cache(reader=read, locate=self.files.get, **kw)

    def test_an_unchanged_transcript_is_parsed_once(self):
        self.raw["a"] = [item("you", "hi"), item("claude", "yo")]
        self.transcript("a")
        c = self.cache()
        first, sig = c.conversation("a")
        for _ in range(5):
            again, sig2 = c.conversation("a")
            c.items("a", limit=1)
        self.assertEqual(self.reads, ["a"])
        self.assertEqual(sig, sig2)
        self.assertEqual(sig, convcache.signature(first))

    def test_an_appended_transcript_is_parsed_again(self):
        self.raw["a"] = [item("you", "hi")]
        self.transcript("a")
        c = self.cache()
        _, sig = c.conversation("a")
        self.raw["a"].append(item("claude", "yo"))
        self.transcript("a", "more")
        items, sig2 = c.conversation("a")
        self.assertEqual(len(items), 2)
        self.assertNotEqual(sig, sig2)
        self.assertEqual(self.reads, ["a", "a"])

    def test_the_limit_applies_before_turns_merge(self):
        # Two claude messages are one turn; a limit of 1 must still be one
        # message, exactly as conversation.read(limit=1) would give.
        self.raw["a"] = [item("you", "q"), item("claude", "one"), item("claude", "two")]
        self.transcript("a")
        c = self.cache()
        self.assertEqual(len(c.conversation("a")[0]), 2)
        self.assertEqual(c.items("a", limit=1), conversation.merge_turns(self.raw["a"][-1:]))
        self.assertEqual(c.items("a"), conversation.merge_turns(self.raw["a"]))

    def test_merging_does_not_change_the_cached_messages(self):
        self.raw["a"] = [item("claude", "one"), item("claude", "two")]
        self.transcript("a")
        c = self.cache()
        c.conversation("a")
        c.items("a", limit=2)
        self.assertEqual(len(c.items("a", limit=1)[0]["blocks"]), 1)

    def test_without_a_transcript_file_it_reads_through(self):
        self.raw["gone"] = [item("you", "hi")]
        c = self.cache()
        c.items("gone")
        c.items("gone")
        self.assertEqual(self.reads, ["gone", "gone"])

    def test_least_recently_used_sessions_are_dropped(self):
        for sid in "abc":
            self.raw[sid] = [item("you", sid)]
            self.transcript(sid)
        c = self.cache(keep=2)
        c.items("a"); c.items("b"); c.items("a"); c.items("c")
        self.reads.clear()
        c.items("a"); c.items("c")
        self.assertEqual(self.reads, [])
        c.items("b")
        self.assertEqual(self.reads, ["b"])

    def test_a_weight_budget_keeps_at_least_the_newest(self):
        self.raw["big"] = [item("you", "x" * 5000)]
        self.transcript("big")
        c = self.cache(budget=100)
        c.items("big"); c.items("big")
        self.assertEqual(self.reads, ["big"])

    def test_concurrent_requests_share_one_parse(self):
        self.raw["a"] = [item("you", "hi")]
        self.transcript("a")
        gate = threading.Event()
        reads = []

        def slow(sid):
            reads.append(sid)
            gate.wait(2)
            return list(self.raw[sid])
        c = convcache.Cache(reader=slow, locate=self.files.get)
        threads = [threading.Thread(target=c.items, args=("a",)) for _ in range(4)]
        [t.start() for t in threads]
        time.sleep(0.2)
        gate.set()
        [t.join() for t in threads]
        self.assertEqual(reads, ["a"])


if __name__ == "__main__":
    unittest.main()
