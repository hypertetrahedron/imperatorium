"""One window: sessions down the side, the selected conversation in the middle.

This replaces a board that told you a session was blocked and a palette that
could not show you what it was blocked on. Seeing the question and answering
it are now the same screen.

Served as a page rather than built in a toolkit: the dispatcher already speaks
HTTP, rendering a conversation is far easier in HTML than in Tk, and the same
page then works on a tablet on the LAN. Open it chromeless with

    msedge --app=http://127.0.0.1:8792/

The shell is static; everything in it is filled from the JSON API, so this
function does no work per request and the page survives the dispatcher
restarting under it.
"""
import html
import json
import os
import re


WEB = os.path.join(os.path.dirname(os.path.abspath(__file__)), "web")


def _web(name):
    """One of the window's source files in web/, read once at import.
    newline="" keeps the bytes as written (they are LF; see .gitattributes).
    The Rust binary compiles the same files in."""
    with open(os.path.join(WEB, name), encoding="utf-8", newline="") as f:
        return f.read()


# The stylesheet every page shares.
CSS = _web("app.css")


# What both pages need - the window and the chat matrix: naming a session's
# state, and rendering what Claude wrote.
SHARED = _web("shared.js")

SCRIPT = SHARED + _web("app.js")


FONTS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fonts")


def font(name):
    """The bytes of a bundled .woff2 named exactly `name`, or None.

    Matched against the folder's own listing, so no request path can reach
    anything outside it.
    """
    try:
        known = set(os.listdir(FONTS))
    except OSError:
        return None
    if not name.endswith(".woff2") or name not in known:
        return None
    with open(os.path.join(FONTS, name), "rb") as f:
        return f.read()


# Every key the window answers to, for the Settings window's list: (where,
# keys, what). Keep it beside the handlers in SCRIPT when either changes. A
# None key is a setting, filled in from that setting's value.
SHORTCUTS = [
    ("Anywhere", "palette.hotkey", "raise or open this window (the first that is free)"),
    ("Prompt box", "Enter", "Send"),
    ("Prompt box", "Shift+Enter", "new line"),
    ("Prompt box", "Ctrl+Enter", "send as a new background session"),
    ("Prompt box", "Alt+Enter", "Queue: deliver when the current turn ends"),
    ("Prompt box", "quick.key", "quick phrases: pick one to put in the box"),
    ("Prompt box", "Esc", "leave the box"),
    ("Window", "Ctrl+K", "jump to a session"),
    ("Window", "J / K", "next / previous session (not while typing)"),
    ("Window", "N, Alt+N", "next session waiting for you"),
    ("Window", "Alt+1 - Alt+9", "the session at that place in the list"),
    ("Window", "/", "filter the session list"),
    ("Window", "F9", "hold to dictate into the prompt box, let go to stop (Settings > Dictation: press once to start, again to stop)"),
    ("Window", "Esc", "close an open menu"),
    ("Conversation", "Ctrl+F", "find in the conversation"),
    ("Conversation", "Enter / Shift+Enter", "in find: earlier / later match"),
    ("Conversation", "Alt+Up / Alt+Down", "your previous / next prompt"),
    ("Lists", "Up / Down, Enter, Esc", "session jump, project picker and quick phrases: "
                                       "choose, take, close"),
    ("Changes", "Ctrl+Enter, Esc", "review comment: add, cancel"),
    ("Settings", "Ctrl+S", "save"),
]


# The sizes the page's own CSS is written at; the Settings window's
# chat.font_size and chat.matrix_font_size scale from these.
FONT_BASE = {"chat": 14, "matrix": 13}


HIGHLIGHTS_ON = ("code", "diffs", "prose", "tools", "signals", "mute", "guides", "shape")
_HEX = re.compile(r"^#[0-9a-fA-F]{6}$")
_WORD = re.compile(r"^[a-z]+$")


def look_head(look):
    """The page's text sizes, highlight switches and colours (settings.look),
    for its <head>: CSS custom properties, and the html.hl-* classes. Every
    highlight is on when no look is given."""
    look = look or {}
    fonts = look.get("fonts") or {}
    props = []
    for name, base in FONT_BASE.items():
        try:
            size = float(fonts.get(name) or base)
        except (TypeError, ValueError):
            size = base
        props.append("--%sz:%s" % (name, round(size / base, 4)))
    for name, v in sorted((look.get("colors") or {}).items()):
        if _WORD.match(str(name)) and _HEX.match(str(v)):
            props.append("--hl-%s:%s" % (name, v.lower()))
    hl = look.get("hl")
    on = [n for n in HIGHLIGHTS_ON if hl is None or hl.get(n)]
    on += [n for n, v in (hl or {}).items() if v and n not in on and _WORD.match(str(n))]
    classes = ("<script>document.documentElement.classList.add(%s)</script>"
               % ",".join(json.dumps("hl-" + n) for n in on)) if on else ""
    return "<style id=lookvars>html:root{%s}</style>%s" % (";".join(props), classes)


def _with_meta(token, page, boot="", look=False):
    """`page` carrying the window token (see Control.send_typed), which
    dispatcher served it (see checkBoot) and, unless `look` is False, what it
    draws with (see look_head)."""
    head = "<meta charset=utf-8>"
    return page.replace(head, head + '<meta name=imp-window content="%s"><meta name=imp-boot content="%s">'
                        '%s'
                        % (html.escape(token or "", quote=True), html.escape(boot or "", quote=True),
                           "" if look is False else look_head(look)), 1)


_MARK = re.compile(r"\{\{(STYLES|SCRIPTS)\}\}")


def _page(name, styles, scripts, token, boot, look):
    """web/<name>.html with its {{STYLES}} and {{SCRIPTS}} marks filled in
    (inline: this server sends one self-contained page) and the per-start
    meta added."""
    fill = {"STYLES": "<style>%s</style>" % "".join(styles),
            "SCRIPTS": "<script>%s</script>" % "".join(scripts)}
    return _with_meta(token, _MARK.sub(lambda m: fill[m.group(1)], _web(name + ".html")), boot, look)


def render_shell(token="", boot="", look=None):
    return _page("shell", [CSS], [SCRIPT], token, boot, look)


# -- the chat matrix ---------------------------------------------------------
# Every running session at once, in its own window: a tile each, holding
# Claude's replies and the one step it is on. The dispatcher does the cutting
# down (see matrix.py); this page only lays the tiles out and fills them.

MATRIX_CSS = _web("matrix.css")
MATRIX_SCRIPT = SHARED + _web("matrix.js")

SETTINGS_CSS = _web("settings.css")
SETTINGS_SCRIPT = SHARED + _web("settings.js")


def render_settings(token="", boot=""):
    return _page("settings", [CSS, SETTINGS_CSS], [SETTINGS_SCRIPT], token, boot, False)


def render_matrix(boot="", look=None):
    return _page("matrix", [CSS, MATRIX_CSS], [MATRIX_SCRIPT], "", boot, look)
