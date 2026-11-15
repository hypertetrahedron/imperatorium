"""`imperatorium sessions` vs the Agent SDK, on every transcript here.

Read-only: it reads ~/.claude/projects (or CLAUDE_CONFIG_DIR) and writes
nothing. For each session it compares

  - list:  sdk.list_sessions() against `sessions list`, row by row;
  - info:  sdk.get_session_info(id) against `sessions info <id>`;
  - chain: the SDK's chain plus conversation.with_compacted, cut to the keys
           the binary keeps, against `sessions chain <id>`;
  - items: conversation.read() through the SDK against through the binary,
           which is what the window actually shows.

A session written to while it is being compared is retried once, then
reported as LIVE rather than as a difference.

    python rust/conformance/sessions_compare.py [--limit N]

--record DIR [--corpus ROOT [--in-place]] [--now T]: Python only, over a
corpus (default the synthetic one in tests/fixtures/corpus; ROOT holds
projects/) with its mtimes pinned, written as goldens (golden.py,
tests/corpus.rs). NEVER record the real ~/.claude into the repository: the
full check copies it outside (see tests/golden/README.md).
"""
import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import REPO, binary  # noqa: E402
from golden import corpus_setup, record_dir, write_json, write_jsonl  # noqa: E402

os.chdir(REPO)
import claude_agent_sdk as sdk  # noqa: E402
from claude_agent_sdk._internal import sessions as s  # noqa: E402

from imperatorium import conversation  # noqa: E402

KEEP = ("type", "uuid", "sessionId", "message", "isMeta", "isSidechain", "teamName",
        "isCompactSummary", "origin", "attachment", "subtype")
INFO_FIELDS = ("session_id", "summary", "last_modified", "file_size", "custom_title",
               "first_prompt", "git_branch", "cwd", "tag", "created_at")


def native(*args):
    done = subprocess.run([binary(), "sessions"] + list(args), capture_output=True, timeout=120)
    if done.returncode != 0:
        raise RuntimeError(done.stderr.decode("utf-8", "replace"))
    return json.loads(done.stdout.decode("utf-8"))


def info_dict(info):
    return None if info is None else {k: getattr(info, k, None) for k in INFO_FIELDS}


def sdk_chain(sid):
    content = s._read_session_file(sid, None)
    if not content:
        return []
    entries = s._parse_transcript_entries(content)
    chain = conversation.with_compacted(entries, s._build_conversation_chain(entries))
    return [{k: e[k] for k in KEEP if k in e} for e in chain]


def items(messages):
    return conversation.to_items(messages, limit=0, merge=False)


def session_files():
    root = s._get_projects_dir()
    for d in sorted(os.listdir(root)):
        full = os.path.join(root, d)
        if not os.path.isdir(full):
            continue
        for name in os.listdir(full):
            if name.endswith(".jsonl") and s._validate_uuid(name[:-6]):
                yield name[:-6], os.path.join(full, name)


def stamp(path):
    try:
        st = os.stat(path)
        return st.st_mtime_ns, st.st_size
    except OSError:
        return None


def first_difference(a, b, path="$"):
    if type(a) is not type(b):
        return "%s: %r vs %r" % (path, type(a).__name__, type(b).__name__)
    if isinstance(a, dict):
        for k in list(a) + [k for k in b if k not in a]:
            if k not in a or k not in b:
                return "%s.%s missing on one side" % (path, k)
            d = first_difference(a[k], b[k], "%s.%s" % (path, k))
            if d:
                return d
        if list(a) != list(b):
            return "%s: key order %r vs %r" % (path, list(a)[:8], list(b)[:8])
        return None
    if isinstance(a, list):
        if len(a) != len(b):
            return "%s: %d vs %d items" % (path, len(a), len(b))
        for i, (x, y) in enumerate(zip(a, b)):
            d = first_difference(x, y, "%s[%d]" % (path, i))
            if d:
                return d
        return None
    return None if a == b else "%s: %r vs %r" % (path, str(a)[:80], str(b)[:80])


def compare_session(sid):
    out = {}
    out["info"] = first_difference(info_dict(sdk.get_session_info(sid)), native("info", sid))
    py_chain = sdk_chain(sid)
    rs_chain = native("chain", sid)
    out["chain"] = first_difference(py_chain, rs_chain)
    py_items = items(conversation._messages(sdk, sid, None))
    rs_items = items(conversation._native_messages(sid))
    out["items"] = first_difference(py_items, rs_items)
    return out, len(rs_chain)


def attempt(fn, *args):
    try:
        return fn(*args)
    except Exception as exc:  # recorded: the binary's answer is not compared
        return {"raises": type(exc).__name__}


def record(folder):
    _, places, _ = corpus_setup()
    write_json(os.path.join(folder, "list.json"),
               places([info_dict(i) for i in sdk.list_sessions()]))
    rows = []
    for sid, _path in session_files():
        rows.append(places({
            "id": sid,
            "info": attempt(lambda: info_dict(sdk.get_session_info(sid))),
            "chain": attempt(sdk_chain, sid),
            "items": attempt(lambda: items(conversation._messages(sdk, sid, None)))}))
    write_jsonl(os.path.join(folder, "sessions.jsonl"), rows)
    print("recorded %d sessions" % len(rows))
    return 0


def main():
    folder = record_dir()
    if folder:
        return record(folder)
    limit = int(sys.argv[sys.argv.index("--limit") + 1]) if "--limit" in sys.argv else None
    failures, live, total, entries = [], [], 0, 0
    t0 = time.time()

    py_list = [info_dict(i) for i in sdk.list_sessions()]
    rs_list = native("list")
    diff = first_difference(py_list, rs_list)
    if diff:
        # A live session's mtime moves between the two reads; retry once.
        py_list = [info_dict(i) for i in sdk.list_sessions()]
        rs_list = native("list")
        diff = first_difference(py_list, rs_list)
    print("list : %d sessions, %s" % (len(rs_list), diff or "identical"))
    if diff:
        failures.append(("list", diff))

    for sid, path in session_files():
        if limit is not None and total >= limit:
            break
        total += 1
        for attempt in (1, 2):
            before = stamp(path)
            result, n = compare_session(sid)
            changed = stamp(path) != before
            bad = {k: v for k, v in result.items() if v}
            if not bad or not changed:
                break
        entries += n
        if bad and changed:
            live.append(sid)
        elif bad:
            failures.append((sid, bad))
    print("sessions: %d compared, %d chain entries, %d live, %.1fs"
          % (total, entries, len(live), time.time() - t0))
    for sid, bad in failures:
        print("DIFF %s %s" % (sid, bad))
    print("sessions_compare: %s" % ("%d differences" % len(failures) if failures else "identical"))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
