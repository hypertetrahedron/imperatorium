"""Decide which permission prompts are worth a human's attention.

At twenty concurrent sessions the bottleneck is not how fast agents work, it
is how many times a day they stop and ask. Most of those asks are for reading
a file or running `git status`. This answers the cheap ones and lets the rest
through to you.

Three rules govern everything here:

  1. **Default to `ask`.** Nothing is allowed unless a rule says so. A tool
     this module has never heard of is a prompt, not a gamble.
  2. **This removes prompts, never answers.** Anything not provably cheap
     becomes `ask` - exactly the prompt you would have got anyway, so you
     keep the power to approve it. `deny` is reserved for one case: an agent
     changing the machinery that decides what it is allowed to do. Refusing
     `git push` or `rm -rf` outright was the first design here and it was
     wrong; it took away your ability to say yes.
  3. **Fail safe.** Any error, any malformed request, any doubt - `ask`.
     A crash in here must cost a prompt, never an approval.

`decide` is pure: request in, verdict out. It does no I/O, so it is cheap to
test exhaustively, which for this file is the point.
"""
import os
import re

ALLOW, DENY, ASK = "allow", "deny", "ask"

# Tools that only observe. Auto-allowing these is the bulk of the win.
READ_ONLY_TOOLS = {"Read", "Glob", "Grep", "NotebookRead", "TodoWrite", "ListAgents"}

# These always name a file. One that does not is malformed, not permissive.
PATH_REQUIRED = {"Read", "NotebookRead"}

# Tools that change the working tree. Allowed only when a project opts in,
# and only inside its own directory.
EDIT_TOOLS = {"Edit", "Write", "NotebookEdit", "MultiEdit"}

# Paths nothing is ever auto-approved to touch - not to read, and certainly
# not to write. Secrets, and the machinery that decides what gets approved.
# The leading class is a boundary, not only a path separator. These have to
# match `C:/proj/.env` and `cat .env` alike, and the second form is how a
# secret actually gets read - anchoring on `/` alone missed it entirely.
_EDGE = r"(^|[\s\\/=:\"'])"
_END = r"($|\s|[\"'])"

SENSITIVE = (
    _EDGE + r"\.env(\.|" + _END + ")",
    _EDGE + r"\.git[\\/]config" + _END,
    _EDGE + r"\.ssh[\\/]",
    _EDGE + r"\.aws[\\/]",
    _EDGE + r"\.npmrc" + _END,
    _EDGE + r"\.netrc" + _END,
    _EDGE + r"id_(rsa|ed25519|ecdsa)",
    r"\.pem" + _END,
    r"\.pfx" + _END,
    r"\.p12" + _END,
    _EDGE + r"credentials(\.|" + _END + ")",
    _EDGE + r"secrets?(\.|[\\/]|" + _END + ")",
)

# The one class refused outright: this system's own controls. An agent must
# never quietly widen what it is allowed to do, and unlike a secret there is
# no legitimate mid-task reason to touch these - you change them yourself.
SELF_PROTECT = (
    _EDGE + r"\.claude[\\/]settings.*\.json" + _END,
    _EDGE + r"policy\.json" + _END,
    _EDGE + r"hooks[\\/]cc_.*\.py" + _END,
    _EDGE + r"ccontrol[\\/]policy\.py" + _END,
    r"--dangerously-skip-permissions",
    r"\bcrossSessionInbound\b",
)

# Bash that is never auto-approved, whatever else matches. These become an
# ordinary prompt: the interrupt is the point, because these are precisely the
# decisions worth your attention.
NEVER_AUTO = (
    (r"\brm\s+(-[a-zA-Z]*\s+)*-[a-zA-Z]*[rf]", "recursive or forced delete"),
    (r"\bgit\s+push\b", "pushes to a remote"),
    (r"\bgit\s+reset\s+--hard\b", "discards work irreversibly"),
    (r"\bgit\s+clean\b", "deletes untracked files"),
    (r"\bgit\s+checkout\s+--\s", "discards local changes"),
    (r"--force\b|--hard\b|\s-f\b", "forcing"),
    (r"\bsudo\b|\brunas\b", "elevates"),
    (r"\b(curl|wget|iwr|Invoke-WebRequest)\b.*\|\s*(sh|bash|pwsh|powershell)",
     "pipes a download into a shell"),
    (r"\b(shutdown|reboot|mkfs|diskpart|format)\b", "touches the machine"),
    (r"\b(npm|pnpm|yarn|pip|uv|cargo|gem)\s+(i|install|add|publish)\b",
     "installs or publishes packages"),
    (r"\bschtasks\b|\bRegister-ScheduledTask\b|\bsc\s+(create|config)\b",
     "changes scheduled tasks or services"),
    (r"\breg\s+(add|delete)\b|\bSet-ItemProperty\b.*HK(LM|CU)", "writes the registry"),
    (r"\bchmod\s+[0-7]*7[0-7]{2}\b", "makes something world-writable"),
    (r">\s*/dev/sd|\bdd\s+if=", "writes a raw device"),
    (r"\bkill\b|\bStop-Process\b|\btaskkill\b", "kills processes"),
    (r"\bclaude\b.*--dangerously-skip-permissions", "disables permission checks"),
)

# Read-only shell. Anchored, because `git status` is safe and
# `git status; rm -rf /` is not - see `_segments`.
SAFE_BASH = (
    r"git\s+(status|diff|log|show|branch|remote|stash\s+list|rev-parse|describe)\b",
    r"git\s+(config\s+--get|ls-files)\b",
    r"(ls|dir|pwd|cd|echo|whoami|hostname|date|uname)\b",
    r"(cat|head|tail|wc|file|stat|find|which|where|type)\b",
    r"(grep|rg|ag|fgrep|egrep)\b",
    r"(python|python3|py)\s+-m\s+(pytest|unittest)\b",
    r"(pytest|tox|nox)\b",
    r"(npm|pnpm|yarn)\s+(test|run\s+(test|lint|typecheck|build))\b",
    # Not `fmt`: formatting rewrites source files, which is an edit.
    r"(cargo|go)\s+(test|build|vet)\b",
    r"dotnet\s+(test|build)\b",
    r"(node|python|python3|py)\s+--version\b",
    r"claude\s+agents\s+--json\b",
)

# A safe verb is only matched at the start of a segment, so its OPTIONS are
# unchecked by SAFE_BASH - and several read-only verbs have options that are
# not. `find . -delete` begins with `find`. Each entry: the verb, a pattern
# that makes that segment unsafe, and why. Found by auditing every verb above
# on 2026-09-24, before auto-approval was first allowed to take effect.
UNSAFE_OPTIONS = (
    (r"find\b", r"\s-(delete|exec|execdir|ok|okdir|fprint0?|fprintf|fls)\b",
     "find that deletes, runs a command, or writes a file"),
    (r"git\s+(diff|log|show)\b", r"\s--output\b", "writes its output to a file"),
    (r"(rg)\b", r"\s--pre\b", "runs a preprocessor program"),
    (r"date\b", r"\s(-s|--set)\b", "sets the clock"),
)

# `git branch` and `git remote` list things with no arguments and change
# things with them, so these are allowed only in their listing forms.
BRANCH_LISTING = {"-a", "--all", "-r", "--remotes", "-v", "-vv", "--verbose",
                  "-l", "--list", "--show-current", "--contains", "--no-contains",
                  "--merged", "--no-merged", "--points-at", "--sort", "--format",
                  "--column", "--no-column", "--color", "--no-color", "-i",
                  "--ignore-case", "--abbrev", "--no-abbrev"}
# Flags after which the next token is a value or pattern, not a branch name.
BRANCH_TAKES_VALUE = {"-l", "--list", "--contains", "--no-contains", "--merged",
                      "--no-merged", "--points-at", "--sort", "--format"}
REMOTE_LISTING = re.compile(
    r"^git\s+remote(\s+(-v|--verbose))?(\s+(show|get-url)(\s+\S+)*)?\s*$", re.I)


def _unsafe_option(segment):
    """Why an otherwise safe-looking segment is not read-only, or None."""
    for verb, pattern, why in UNSAFE_OPTIONS:
        if re.match(verb, segment, re.I) and re.search(pattern, segment, re.I):
            return why
    if re.match(r"git\s+remote\b", segment, re.I) and not REMOTE_LISTING.match(segment):
        return "changes a git remote"
    if re.match(r"git\s+branch\b", segment, re.I):
        tokens = segment.split()[2:]
        expect_value = False
        for token in tokens:
            if expect_value:
                expect_value = False
                continue
            name = token.split("=", 1)[0]
            if name in BRANCH_LISTING:
                expect_value = name in BRANCH_TAKES_VALUE and "=" not in token
                continue
            return "creates, deletes or renames a branch"
    return None


# Anything that chains, substitutes, or redirects has to be taken apart before
# it can be trusted.
_SPLIT = re.compile(r"&&|\|\||\||;|\n")
_SUBSHELL = re.compile(r"\$\(|`|<\(")
_REDIRECT = re.compile(r"(^|\s)>{1,2}(\s|$)|(^|\s)\d?>&")


def _matches_any(patterns, text):
    for pattern in patterns:
        if re.search(pattern, text, re.IGNORECASE):
            return pattern
    return None


def is_sensitive(path):
    """True for a path no rule may auto-approve (asked about, not refused)."""
    if not path:
        return False
    return bool(_matches_any(SENSITIVE, str(path).replace("\\", "/")))


def is_self_protected(text):
    """True for this system's own controls, which are refused outright."""
    if not text:
        return False
    return bool(_matches_any(SELF_PROTECT, str(text).replace("\\", "/")))


def _segments(command):
    """Split a shell command into the pieces that each have to be safe."""
    return [part.strip() for part in _SPLIT.split(command or "") if part.strip()]


def _inside(path, root):
    """True when `path` resolves inside `root`."""
    if not path or not root:
        return False
    try:
        path_abs = os.path.normcase(os.path.abspath(str(path)))
        root_abs = os.path.normcase(os.path.abspath(str(root)))
    except (OSError, ValueError):
        return False
    return path_abs == root_abs or path_abs.startswith(root_abs + os.sep)


def _bash_verdict(command, policy):
    if not command or not command.strip():
        return ASK, "empty command", "unknown"

    # Substitution can hide anything inside an otherwise innocent line.
    if _SUBSHELL.search(command):
        return ASK, "uses command substitution", "opaque"
    if _REDIRECT.search(command):
        return ASK, "redirects output", "mutating"

    for pattern, why in NEVER_AUTO:
        if re.search(pattern, command, re.IGNORECASE):
            return ASK, why, "consequential"

    segments = _segments(command)
    if not segments:
        return ASK, "nothing to check", "unknown"

    extra = tuple(policy.get("safe_bash") or ())
    for segment in segments:
        # Strip leading environment assignments, as in `FOO=1 pytest`.
        stripped = re.sub(r"^(\w+=\S*\s+)+", "", segment).strip()
        # The verb has to START the segment: `git status` is safe, and
        # `foo --flag=git status` is not the same thing at all.
        if not _begins_safely(stripped, SAFE_BASH + extra):
            return ASK, "not a known read-only command: %s" % stripped[:60], "unknown"
        why = _unsafe_option(stripped)
        if why:
            return ASK, why, "consequential"
    return ALLOW, "read-only shell", "read"


def _begins_safely(segment, patterns):
    for pattern in patterns:
        if re.match(pattern, segment, re.IGNORECASE):
            return True
    return False


def _paths_in(tool_input):
    for key in ("file_path", "path", "notebook_path", "filePath"):
        value = tool_input.get(key)
        if value:
            yield value
    for value in tool_input.get("file_paths") or ():
        yield value
    for edit in tool_input.get("edits") or ():
        if isinstance(edit, dict) and edit.get("file_path"):
            yield edit["file_path"]


def decide(request, policy=None):
    """Return (verdict, reason, tier) for one PermissionRequest.

    `request` is the hook payload: tool_name, tool_input, cwd, permission_mode.
    Anything unexpected returns ASK - this function never raises.
    """
    policy = policy or {}
    try:
        return _decide(request, policy)
    except Exception as exc:  # deliberately broad: a bug here must cost a prompt
        return ASK, "policy error (%s)" % type(exc).__name__, "error"


def _decide(request, policy):
    if not policy.get("enabled", False):
        return ASK, "policy disabled", "off"

    tool = request.get("tool_name") or ""
    tool_input = request.get("tool_input")
    if not isinstance(tool_input, dict):
        tool_input = {}
    cwd = request.get("cwd") or ""

    project = _project_policy(policy, cwd)
    if project.get("never_auto_approve"):
        return ASK, "project opts out", "off"

    # Checked before anything else. Self-protection is the only outright
    # refusal; a secret is merely never auto-approved.
    for path in _paths_in(tool_input):
        if is_self_protected(path):
            return DENY, "would change this system's own controls", "self"
        if is_sensitive(path):
            return ASK, "names a secret: %s" % os.path.basename(str(path)), "secret"

    if tool in ("Bash", "PowerShell"):
        command = tool_input.get("command") or ""
        if is_self_protected(command):
            return DENY, "would change this system's own controls", "self"
        if _matches_any(SENSITIVE, command.replace("\\", "/")):
            return ASK, "command names a secret", "secret"
        return _bash_verdict(command, {**policy, **project})

    if tool in READ_ONLY_TOOLS:
        paths = list(_paths_in(tool_input))
        if tool in PATH_REQUIRED and not paths:
            return ASK, "%s without a path" % tool, "unknown"
        for path in paths:
            if not _inside(path, cwd) and not project.get("allow_reads_outside_cwd"):
                return ASK, "reads outside the project", "read"
        return ALLOW, "read-only tool", "read"

    if tool in EDIT_TOOLS:
        if not project.get("auto_edit"):
            return ASK, "edits need auto_edit for this project", "write"
        paths = list(_paths_in(tool_input))
        if not paths:
            return ASK, "edit with no path", "write"
        for path in paths:
            if not _inside(path, cwd):
                return ASK, "edits outside the project", "write"
        return ALLOW, "edit inside an opted-in project", "write"

    return ASK, "no rule for %s" % (tool or "an unnamed tool"), "unknown"


def _project_policy(policy, cwd):
    """The `projects` entry whose path contains cwd, merged over defaults."""
    merged = dict(policy.get("defaults") or {})
    best, best_len = None, -1
    for path, settings in (policy.get("projects") or {}).items():
        if _inside(cwd, path) and len(str(path)) > best_len:
            best, best_len = settings, len(str(path))
    if isinstance(best, dict):
        merged.update(best)
    return merged
