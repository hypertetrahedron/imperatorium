"""Projects, and turning something a human said into one of them.

A project is a directory. The catalogue is built from configured roots plus
every working directory Claude Code has actually been run in, so a project
that lives outside the roots still resolves once it has been worked in.

Matching is deliberately conservative. A wrong match here does not garble
text - it fires a prompt at the wrong repository - so `resolve` reports its
confidence and the caller is expected to confirm anything short of certain.
"""
import difflib
import json
import os
import re

CONFIG_NAME = "config.json"
# Where projects live when config.json does not say (`"roots": [...]`): the
# folder this checkout sits in, since repositories are usually cloned side by
# side. On the machine this was built on that is Y:/projects-software.
DEFAULT_ROOTS = [os.path.dirname(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__))))]
# Directories that are never a project of their own.
SKIP = {".git", ".venv", "venv", "node_modules", "__pycache__", ".idea", ".vs"}

# Above this, act. Below `MAYBE`, refuse rather than guess.
CERTAIN = 0.87
MAYBE = 0.60


def _norm(text):
    """Fold to comparable words: 'laser-ledger' and 'Laser Ledger' agree."""
    text = re.sub(r"[_\-]+", " ", text or "")
    text = re.sub(r"(?<=[a-z0-9])(?=[A-Z])", " ", text)
    text = re.sub(r"[^\w ]+", " ", text)
    return " ".join(text.lower().split())


def load_config(project_dir):
    path = os.path.join(project_dir, CONFIG_NAME)
    try:
        with open(path, encoding="utf-8") as fh:
            return json.load(fh)
    except (OSError, ValueError):
        return {}


def claude_home():
    return os.environ.get("CLAUDE_CONFIG_DIR") or os.path.join(
        os.path.expanduser("~"), ".claude"
    )


def known_cwds():
    """Every directory Claude Code has a transcript folder for.

    The folder name is the absolute path with non-alphanumerics replaced by
    '-', which is lossy and not reversible, so these contribute *names* to
    the catalogue, not paths. On Windows the same path can appear under two
    different cases; fold them together.
    """
    names = set()
    d = os.path.join(claude_home(), "projects")
    try:
        entries = os.listdir(d)
    except OSError:
        return names
    for slug in entries:
        leaf = slug.rstrip("-").split("-")[-1]
        if leaf and leaf not in SKIP:
            names.add(leaf.lower())
    return names


def discover(roots=None, extra=None):
    """The project catalogue: {name, path, source}."""
    roots = roots or DEFAULT_ROOTS
    seen = {}
    for root in roots:
        try:
            entries = sorted(os.listdir(root))
        except OSError:
            continue
        for name in entries:
            if name in SKIP or name.startswith("."):
                continue
            path = os.path.join(root, name)
            if not os.path.isdir(path):
                continue
            key = os.path.normcase(os.path.abspath(path))
            seen[key] = {"name": name, "path": os.path.abspath(path), "source": "root"}
    for path in extra or []:
        if not path:
            continue
        key = os.path.normcase(os.path.abspath(path))
        if key not in seen:
            seen[key] = {
                "name": os.path.basename(os.path.normpath(path)),
                "path": os.path.abspath(path),
                "source": "session",
            }
    out = list(seen.values())
    out.sort(key=lambda p: p["name"].lower())
    return out


def rank(query, catalogue, limit=12):
    """Catalogue ordered for a live type-ahead list.

    Shares `_norm` and the same similarity measure as `resolve`, so the entry
    the palette highlights is the one `resolve` would have picked. An empty
    query keeps the catalogue's own order rather than scoring everything the
    same and shuffling it.
    """
    target = _norm(query)
    if not target:
        return list(catalogue)[:limit]
    scored = []
    for proj in catalogue:
        name = _norm(proj["name"])
        score = difflib.SequenceMatcher(None, target, name).ratio()
        if name == target:
            score = 1.0
        elif name.startswith(target):
            score = max(score, 0.95)
        elif target in name:
            score = max(score, 0.9)
        # Subsequence match, so "blp" still finds "blue-lamp-project".
        elif _subsequence(target.replace(" ", ""), name.replace(" ", "")):
            score = max(score, 0.7)
        scored.append((score, proj))
    scored.sort(key=lambda pair: (-pair[0], pair[1]["name"].lower()))
    return [p for s, p in scored if s > 0.3][:limit]


def _subsequence(needle, haystack):
    it = iter(haystack)
    return all(char in it for char in needle)


def resolve(spoken, catalogue):
    """Best project for `spoken`, with a confidence and the runners-up.

    Returns {match, score, confidence, candidates}. `confidence` is one of
    'certain', 'maybe', 'none' - the caller decides what to do with anything
    that is not 'certain'.
    """
    target = _norm(spoken)
    scored = []
    if target:
        for proj in catalogue:
            name = _norm(proj["name"])
            score = difflib.SequenceMatcher(None, target, name).ratio()
            # Reward a clean prefix or containment: "laser ledger" said for
            # "laser-ledger-app" should not lose to a shorter unrelated name.
            if name == target:
                score = 1.0
            elif name.startswith(target) or target.startswith(name):
                score = max(score, 0.93)
            elif target in name or name in target:
                score = max(score, 0.88)
            scored.append((score, proj))
    scored.sort(key=lambda pair: (-pair[0], pair[1]["name"].lower()))
    top = scored[:5]
    best_score, best = top[0] if top else (0.0, None)
    runner = top[1][0] if len(top) > 1 else 0.0

    if best_score >= CERTAIN and best_score - runner >= 0.03:
        confidence = "certain"
    elif best_score >= MAYBE:
        confidence = "maybe"
    else:
        confidence = "none"
    return {
        "match": best if confidence != "none" else None,
        "score": round(best_score, 3),
        "confidence": confidence,
        "candidates": [
            {"name": p["name"], "path": p["path"], "score": round(s, 3)} for s, p in top
        ],
    }


PREFIX = re.compile(
    r"^\s*(?:for\s+)?projects?\s*[:,]?\s*(?P<name>.+?)\s*[:,]\s*(?P<prompt>.+)$",
    re.IGNORECASE | re.DOTALL,
)


def parse_utterance(text):
    """Split 'project: laser ledger, do the thing' into name and prompt.

    Returns (name, prompt); name is None when the text carries no project
    prefix, in which case the whole string is the prompt.
    """
    m = PREFIX.match(text or "")
    if not m:
        return None, (text or "").strip()
    return m.group("name").strip(), m.group("prompt").strip()


if __name__ == "__main__":
    import sys

    cat = discover()
    print("%d projects\n" % len(cat))
    said = " ".join(sys.argv[1:])
    if said:
        name, prompt = parse_utterance(said)
        print("heard project : %r" % name)
        print("heard prompt  : %r" % prompt)
        res = resolve(name or said, cat)
        print("confidence    : %s (%.3f)" % (res["confidence"], res["score"]))
        for c in res["candidates"]:
            print("   %-32s %.3f" % (c["name"], c["score"]))
