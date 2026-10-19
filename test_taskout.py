"""Finding and reading the output files of what a session is waiting on."""
import json
import os
import shutil
import tempfile
import time
import unittest

from ccontrol import taskout

SID = "11111111-2222-3333-4444-555555555555"


class Base(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.mkdtemp()
        self.folder = os.path.join(self.root, "Y--proj", SID, "tasks")
        os.makedirs(self.folder)

    def tearDown(self):
        shutil.rmtree(self.root, ignore_errors=True)

    def write(self, task_id, text):
        path = os.path.join(self.folder, task_id + ".output")
        with open(path, "w", encoding="utf-8", newline="") as fh:
            fh.write(text)
        return path


class Folder(Base):
    def test_found_by_session_id_whatever_the_project_slug(self):
        self.assertEqual(taskout.tasks_dir(SID, roots=[self.root]), self.folder)

    def test_unknown_or_malformed_ids_find_nothing(self):
        self.assertIsNone(taskout.tasks_dir("0" * 8 + SID[8:], roots=[self.root]))
        self.assertIsNone(taskout.tasks_dir("../x", roots=[self.root]))


class Tail(Base):
    def test_colour_codes_and_redrawn_lines_are_cleaned(self):
        path = self.write("b1", "\x1b[32mok\x1b[0m\r\n10%\r50%\r100%\ndone\n")
        text, size, mtime = taskout.tail(path)
        self.assertEqual(text, "ok\n100%\ndone\n")
        self.assertGreater(size, 0)
        self.assertIsNotNone(mtime)

    def test_a_long_file_is_cut_at_a_line(self):
        path = self.write("b1", "".join("line %d\n" % i for i in range(1000)))
        text, _, _ = taskout.tail(path, limit=100)
        self.assertTrue(text.startswith("line "))
        self.assertTrue(text.endswith("line 999\n"))
        self.assertLessEqual(len(text), 100)

    def test_a_vanished_file_reads_as_empty(self):
        self.assertEqual(taskout.tail(os.path.join(self.folder, "no.output")),
                         ("", 0, None))

    def test_reading_leaves_the_writer_writing(self):
        path = os.path.join(self.folder, "b2.output")
        with open(path, "w", encoding="utf-8") as writer:
            writer.write("one\n"); writer.flush()
            self.assertEqual(taskout.tail(path)[0], "one\n")
            writer.write("two\n"); writer.flush()
            self.assertEqual(taskout.tail(path)[0], "one\ntwo\n")


class Awaited(Base):
    def test_background_shells_and_monitors_by_their_ids(self):
        self.write("bshell", "building\n")
        self.write("bmon", "event\n")
        stat = {"shells": [{"id": "bshell", "label": "make", "started": 1}],
                "monitors": [{"id": "bmon", "label": "watch", "started": 2}],
                "agents": [{"id": "aagent"}], "known_tasks": ["bshell", "bmon", "aagent"]}
        got = taskout.awaited(stat, self.folder)
        self.assertEqual([(t["id"], t["kind"], t["text"]) for t in got],
                         [("aagent", "agent", ""), ("bshell", "shell", "building\n"),
                          ("bmon", "monitor", "event\n")])

    def test_a_foreground_call_gets_the_file_no_record_names(self):
        self.write("bold", "finished long ago\n")      # a named, ended task
        self.write("aagent", "")                        # a subagent's
        self.write("bfg", "compiling\n")
        stat = {"shells": [{"id": "toolu_1", "label": "cargo build", "foreground": True,
                            "started": time.time() - 1}],
                "known_tasks": ["bold"]}
        got = taskout.awaited(stat, self.folder)
        self.assertEqual(len(got), 1)
        self.assertEqual((got[0]["id"], got[0]["text"], got[0]["foreground"]),
                         ("bfg", "compiling\n", True))

    def test_a_file_older_than_the_call_is_not_its_output(self):
        path = self.write("bstale", "old\n")
        os.utime(path, (time.time() - 3600,) * 2)
        stat = {"shells": [{"id": "toolu_1", "label": "x", "foreground": True,
                            "started": time.time() + 3600}]}
        got = taskout.awaited(stat, self.folder)
        self.assertEqual((got[0]["id"], got[0]["text"]), ("toolu_1", ""))

    def test_no_folder_still_lists_the_tasks(self):
        stat = {"shells": [{"id": "b1", "label": "x"}]}
        self.assertEqual([t["text"] for t in taskout.awaited(stat, None)], [""])



def entry(kind, *blocks, parent="p"):
    return {"type": kind, "parentUuid": parent, "message": {"content": list(blocks)}}


class Subagents(Base):
    def setUp(self):
        super().setUp()
        self.agents = os.path.join(self.root, "Y--proj", SID, "subagents")
        os.makedirs(self.agents)

    def agent(self, agent_id, entries, call=None):
        path = os.path.join(self.agents, "agent-%s.jsonl" % agent_id)
        with open(path, "w", encoding="utf-8", newline="") as fh:
            fh.write("".join(json.dumps(e) + "\n" for e in entries))
        if call:
            with open(path[:-len(".jsonl")] + ".meta.json", "w", encoding="utf-8") as fh:
                json.dump({"toolUseId": call, "description": "x"}, fh)
        return path

    def test_folder_is_beside_the_transcript(self):
        transcript = os.path.join(self.root, "Y--proj", SID + ".jsonl")
        self.assertEqual(taskout.subagents_dir(transcript), self.agents)
        self.assertIsNone(taskout.subagents_dir(None))
        self.assertIsNone(taskout.subagents_dir(os.path.join(self.root, "none.jsonl")))

    def test_files_found_by_agent_id_and_by_the_call_that_started_them(self):
        path = self.agent("a1", [], call="toolu_9")
        self.assertEqual(taskout.agent_files(self.agents), {"a1": path, "toolu_9": path})
        self.assertEqual(taskout.agent_files(None), {})

    def test_story_tells_text_calls_and_failures_but_not_the_task(self):
        text = taskout.story([
            entry("user", {"type": "text", "text": "the task"}, parent=None),
            entry("assistant", {"type": "text", "text": "Looking.\n"},
                  {"type": "tool_use", "name": "Grep", "input": {"pattern": "foo"}}),
            entry("user", {"type": "tool_result", "content": "fine"}),
            entry("assistant", {"type": "tool_use", "name": "Bash", "input": {"command": "make\nmore"}}),
            entry("user", {"type": "tool_result", "is_error": True,
                           "content": [{"type": "text", "text": "exit 2\nlong trace"}]}),
        ])
        self.assertEqual(text, "Looking.\n> Grep foo\n> Bash make\n  x exit 2\n")

    def test_a_cut_tail_drops_the_partial_first_line(self):
        path = self.agent("a1", [entry("assistant", {"type": "text", "text": "x" * 300}),
                                 entry("assistant", {"type": "text", "text": "last"})])
        text, size, _ = taskout.agent_tail(path, limit=200)
        self.assertEqual(text, "last\n")
        self.assertEqual(size, os.path.getsize(path))

    def test_background_and_foreground_subagents_get_their_transcripts(self):
        self.agent("abg", [entry("assistant", {"type": "text", "text": "bg"})])
        self.agent("afg", [entry("assistant", {"type": "text", "text": "fg"})], call="toolu_1")
        stat = {"agents": [{"id": "abg", "label": "review", "started": 1},
                           {"id": "toolu_1", "label": "explore", "foreground": True, "started": 2}]}
        got = taskout.awaited(stat, None, agents=self.agents)
        self.assertEqual([(t["id"], t["kind"], t["text"], t["foreground"]) for t in got],
                         [("abg", "agent", "bg\n", False), ("toolu_1", "agent", "fg\n", True)])


if __name__ == "__main__":
    unittest.main()
