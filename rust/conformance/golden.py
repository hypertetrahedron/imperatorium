"""Recording Python's answers as goldens for the Rust integration tests.

Each conformance script takes `--record DIR`: it then runs the Python side
only and writes what Python answered, normalised exactly as the script
normalises before comparing, as JSON under DIR (normally
rust/imperatorium/tests/golden/<part>/). The Rust tests in
rust/imperatorium/tests/ replay the same inputs against the binary.

Machine-specific strings are replaced by placeholders, which the Rust side
puts back with its own values (tests/common/mod.rs, `Places`):

    <repo>    the checkout (this file's ../..), and <repo-parent>
    <tmp>     the temporary folder a script works in
    <home>    the user's home folder

each in four spellings: as is, lower-cased (normcase), with forward slashes,
and both - <repo>, <repo-lc>, <repo/>, <repo/-lc> - and, where JSON escaping
changes a spelling (backslashes doubled, inside a body or a stdout kept as
text), <j:repo> and so on, and percent-encoded (quote(v, safe=""), a deep
link) as <u:repo>. Replacement is on decoded strings, longest first.

Goldens must be byte-identical from one capture to the next: run a capture
twice into two folders and compare them (see tests/golden/README.md).
"""
import json
import os
import shutil
import sys
import urllib.parse

HERE = os.path.dirname(os.path.abspath(__file__))
RUST = os.path.dirname(HERE)
REPO = os.path.dirname(RUST)
GOLDEN = os.path.join(RUST, "imperatorium", "tests", "golden")
CORPUS = os.path.join(RUST, "imperatorium", "tests", "fixtures", "corpus")
# Every corpus file's mtime, in sorted relative-path order: BASE + STEP * i.
MTIME_BASE, MTIME_STEP = 1767225600, 60


def record_dir(argv=None):
    """The folder after --record, or None."""
    argv = sys.argv if argv is None else argv
    if "--record" not in argv:
        return None
    path = os.path.abspath(argv[argv.index("--record") + 1])
    os.makedirs(path, exist_ok=True)
    return path


def spellings(text):
    out = []
    for v in (text, os.path.normcase(text), text.replace("\\", "/"),
              os.path.normcase(text).replace("\\", "/")):
        if v not in out:
            out.append(v)
    return out


class Places:
    """Placeholders for this machine's paths (see the module doc)."""

    def __init__(self, **extra):
        bases = [("repo", REPO), ("repo-parent", os.path.dirname(REPO)),
                 ("home", os.path.expanduser("~"))]
        bases += sorted(extra.items())
        pairs = []
        for name, base in bases:
            tags = ["<%s>" % name, "<%s-lc>" % name, "<%s/>" % name, "<%s/-lc>" % name]
            variants = [base, os.path.normcase(base), base.replace("\\", "/"),
                        os.path.normcase(base).replace("\\", "/")]
            for tag, v in zip(tags, variants):
                pairs.append((v, tag))
                # As it appears inside JSON text (a hook's body, a stdout):
                # backslashes doubled. <j:...> puts back the escaped form.
                escaped = json.dumps(v, ensure_ascii=False)[1:-1]
                if escaped != v:
                    pairs.append((escaped, "<j:" + tag[1:]))
                # Percent-encoded as urllib's quote(v, safe="") does (a deep
                # link's cwd): <u:...>.
                quoted = urllib.parse.quote(v, safe="")
                if quoted != v:
                    pairs.append((quoted, "<u:" + tag[1:]))
        # Longest first, so the checkout wins over its parent and the home.
        seen = set()
        self.pairs = []
        for v, tag in sorted(pairs, key=lambda p: -len(p[0])):
            if v not in seen:
                seen.add(v)
                self.pairs.append((v, tag))

    def text(self, s):
        for v, tag in self.pairs:
            if v in s:
                s = s.replace(v, tag)
        return s

    def __call__(self, value):
        """`value` with every string (dict keys too) neutralised."""
        if isinstance(value, str):
            return self.text(value)
        if isinstance(value, dict):
            return {self.text(k): self(v) for k, v in value.items()}
        if isinstance(value, (list, tuple)):
            return [self(v) for v in value]
        return value


def dumps(value):
    return json.dumps(value, ensure_ascii=False)


def write_json(path, value):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        json.dump(value, fh, ensure_ascii=False, indent=1)
        fh.write("\n")


def write_jsonl(path, values):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        for v in values:
            fh.write(dumps(v) + "\n")


def write_bytes(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as fh:
        fh.write(data)


class Recorder:
    """A twin's --record: the steps it took and what the Python twin
    answered, each normalised as the twin normalises before comparing.

    While recording, a twin runs Python on BOTH sides (the "rs" side is a
    second Python dispatcher), so every comparison the script makes still
    runs - Python against itself, which catches nondeterminism in the run -
    and the Python side's values are written as the goldens."""

    def __init__(self, folder, **places):
        self.folder = folder
        self.places = Places(**places)
        self.steps = []
        self.extra = []

    def add(self, step, **fields):
        self.steps.append(dict({"step": step}, **fields))

    def save(self, name="steps.jsonl", subst=None):
        """Write the steps; `subst` maps further run-specific text (a pipe
        name's random tag, a pid) to a placeholder, on the JSON text."""
        rows = [self.places(s) for s in self.steps]
        if subst:
            out = []
            for r in rows:
                text = dumps(r)
                for real, tag in sorted(subst.items(), key=lambda kv: -len(kv[0])):
                    text = text.replace(real, tag)
                out.append(json.loads(text))
            rows = out
        write_jsonl(os.path.join(self.folder, name), rows)
        print("recorded %d steps" % len(rows))

    def file(self, name, data):
        """A fixture the replay needs (a policy, a config), as recorded."""
        path = os.path.join(self.folder, name)
        if isinstance(data, (bytes, bytearray)):
            write_bytes(path, data)
        else:
            write_json(path, self.places(data))


REC = None


def start_recording(**places):
    """Called by a twin's main(): a Recorder when --record was given."""
    global REC
    folder = record_dir()
    REC = Recorder(folder, **places) if folder else None
    return REC


def recording():
    return REC is not None


def rec(step, **fields):
    if REC is not None:
        REC.add(step, **fields)


def python_dispatcher(port, state):
    """The command a recording twin runs in place of the binary."""
    return [sys.executable, "-m", "imperatorium.dispatcher", "--port", str(port), "--state", state]


def flag(name, default=None, argv=None):
    argv = sys.argv if argv is None else argv
    return argv[argv.index(name) + 1] if name in argv else default


# The clocks stats are folded at: before everything, the corpus' "now" and
# two hours on (cache warm/cold, Monitor and wakeup expiry), and far ahead.
SYNTHETIC_NOW = 1767229200.0   # 2026-01-01T01:00Z, an hour into the corpus


def clocks(now):
    return [1.0, now, now + 7200.0, 4.0e9]


def corpus_setup():
    """For a corpus check's --record: the corpus to read (--corpus DIR, a
    folder holding projects/; default the synthetic one in the repo),
    prepared with pinned mtimes and made CLAUDE_CONFIG_DIR.

    The synthetic corpus is copied to a temporary folder first; a local one
    (--in-place, the frozen copy outside the repo) is pinned where it is.
    Returns (root, places, now). `places` leaves values as they are: a
    transcript's own text may hold "<repo>" or "<home>" literally (the real
    corpus does), and nothing a reader returns names the corpus' folder
    (a copy in a random temp folder would make two captures differ)."""
    import tempfile
    src = os.path.abspath(flag("--corpus", CORPUS))
    if "--in-place" in sys.argv:
        root = pin_mtimes(src)
    else:
        import atexit
        tmp = tempfile.mkdtemp(prefix="imp-golden-corpus-")
        atexit.register(shutil.rmtree, tmp, True)
        root = corpus_copy(src, os.path.join(tmp, "corpus"))
    os.environ["CLAUDE_CONFIG_DIR"] = root
    now = float(flag("--now", SYNTHETIC_NOW))
    return root, (lambda value: value), now


def pin_mtimes(root):
    files = []
    for d, _, names in os.walk(os.path.join(root, "projects")):
        for n in names:
            full = os.path.join(d, n)
            files.append((os.path.relpath(full, root).replace("\\", "/"), full))
    for i, (_, full) in enumerate(sorted(files)):
        t = MTIME_BASE + MTIME_STEP * i
        os.utime(full, (t, t))
    return root


def corpus_copy(src, dest):
    """A copy of a corpus folder (its projects/ inside) with every file's
    mtime pinned, so the session list's order and last_modified are fixed.
    Returns the copy's root, for CLAUDE_CONFIG_DIR."""
    if os.path.exists(dest):
        shutil.rmtree(dest)
    shutil.copytree(src, dest)
    return pin_mtimes(dest)
