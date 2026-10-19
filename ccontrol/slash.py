"""Run a slash command in a local session by typing it at its prompt.

A slash command sent through the inbox arrives as inert text: Claude Code
treats every inbox message as a peer's, and a peer cannot run commands (see
inbox.py). Typing into the session's console is real terminal input, so the
command runs as if typed there - the same route `closing` uses for `/exit`.

Real input carries real authority, which is what the guards are for:

  * Only into a session that is idle at its prompt with nothing parked for
    a decision. Typed text and Enter would answer a permission prompt, and
    would be mixed into the input of a turn that is running.
  * Ctrl+C first, to clear a half-typed draft that would otherwise run
    together with the command. At an idle prompt it does nothing else; the
    idle check is made again just before typing, so it cannot land on a turn.
  * One line only: a newline would submit whatever came before it.
  * Commands that open a menu in the terminal are refused - the window
    cannot drive one, and it would sit open in the terminal, unseen.

Local Windows sessions only: a remote session has no console to attach to
here, and macOS and Linux would need a terminal multiplexer to type into.
"""
from . import closing

# Open a menu or panel in the terminal whatever follows them.
MENU = {
    "agents", "allowed-tools", "bashes", "config", "doctor", "help", "hooks",
    "ide", "login", "logout", "mcp", "memory", "permissions", "plugin",
    "plugins", "privacy-settings", "settings", "status", "tasks",
    "terminal-setup", "theme", "upgrade",
}
# Open one only when given nothing to act on.
MENU_BARE = {
    "add-dir", "checkpoint", "continue", "export", "model", "output-style",
    "resume", "rewind",
}
# Ending the session is the window's Close, which also closes its tab.
EXIT = {"exit", "quit"}


def parse(text):
    """(name, rest) for a slash command, else None. `name` is lowercased."""
    text = (text or "").strip()
    if not text.startswith("/") or len(text) < 2 or text[1] in "/ ":
        return None
    name, _, rest = text[1:].partition(" ")
    return name.lower(), rest.strip()


def is_command(text):
    return parse(text) is not None


def refusal(text):
    """Why `text` cannot be typed at a prompt, or None if it can."""
    got = parse(text)
    if got is None:
        return "not a slash command"
    name, rest = got
    if "\n" in text.strip() or "\r" in text.strip():
        return "a slash command is one line"
    if name in EXIT:
        return "use Close to end the session - it closes the terminal tab too"
    if name in MENU or (name in MENU_BARE and not rest):
        hint = " - give it an argument (e.g. /%s <name>)" % name if name in MENU_BARE else ""
        return "/%s opens a menu in the terminal, which the window cannot " \
               "drive; use Focus and run it there%s" % (name, hint)
    return None


def unreachable(row):
    """Why `row` can never be typed into from here, or None."""
    if row.get("remote"):
        return "slash commands can only be typed into a session on this " \
               "machine, not one on %s" % row.get("host")
    if not closing.WINDOWS:
        return "typing a slash command into a session is Windows-only for now"
    return None


def blocked(row, parked=()):
    """Why `row` cannot be typed into right now, or None."""
    why = unreachable(row)
    if why:
        return why
    if not isinstance(row.get("pid"), int):
        return "that session is not running"
    if any(p.get("session_id") == row.get("sessionId") for p in parked):
        return "it is waiting on a permission decision - answer that first"
    status = row.get("status")
    if status == "waiting":
        return "it is waiting on you in the terminal - answer that first"
    if status != "idle":
        return "it is working - send the command when its turn ends, or queue it"
    return None


def type_command(pid, text, typer=None):
    """Clear the prompt and type `text` and Enter into `pid`. True on success."""
    typer = typer or closing._type
    return typer(pid, closing.CTRL_C, text.strip(), closing.ENTER)


# Send As Typed: an ordinary prompt typed at the prompt, so it arrives as the
# person's own input rather than a peer's. Off unless the person turns it on
# in Settings, and only the window may ask for it (see Control.send_typed).

# Long enough for any prompt worth typing; the typer passes the text on a
# child's command line, which Windows caps at 32,767 characters.
TYPED_MAX = 8000
# Line and paragraph breaks: Enter would submit what came before, so they
# become spaces.
_BREAKS = "\r\n\t\v\f\x85  "
# At the very start of the prompt these switch the input mode instead of
# being text: `!` runs a shell command, `#` was the memory shortcut.
_MODE_KEYS = "!#"


def clean_typed(text):
    """`text` made safe to type as one prompt: (clean, changed).

    Breaks become spaces, other control and invisible format characters go,
    runs of spaces collapse, and a leading `!` or `#` is dropped so the
    prompt cannot switch the input into shell mode.
    """
    import unicodedata
    raw = text or ""
    out = []
    for ch in raw:
        if ch in _BREAKS:
            out.append(" ")
        elif unicodedata.category(ch) in ("Cc", "Cf") and ch != "‍":
            continue  # ‍ joins emoji sequences, which are text
        else:
            out.append(ch)
    clean = " ".join("".join(out).split())
    if not is_command(clean):
        clean = clean.lstrip(_MODE_KEYS).lstrip()
    return clean, clean != raw.strip()


def typed_refusal(clean):
    """Why cleaned text cannot be sent as typed, or None."""
    if not clean:
        return "nothing left to type"
    if len(clean) > TYPED_MAX:
        return "too long to type (%d characters, at most %d) - use Send" % (
            len(clean), TYPED_MAX)
    if is_command(clean):
        return refusal(clean)
    return None
