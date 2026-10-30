"""The window's text rendering, run under node against the real SCRIPT.

The renderer is JavaScript inside ui.py, so it is exercised by slicing the
Markdown and table functions out of the shipped string rather than by a copy that could
drift from it. Skipped where node is not installed.
"""
import json
import shutil
import subprocess
import time
import unittest

from imperatorium import ui

NODE = shutil.which("node")


def render(text):
    start = ui.SCRIPT.index("const esc =")
    esc = ui.SCRIPT[start:ui.SCRIPT.index("\n\n", start)]
    body = ui.SCRIPT[ui.SCRIPT.index("const TABLE_SEP"):ui.SCRIPT.index("function renderBlocks")]
    js = esc + "\n" + body + "\nprocess.stdout.write(renderText(%s));" % json.dumps(text)
    out = subprocess.run([NODE, "-"], input=js, capture_output=True, text=True,
                         encoding="utf-8", timeout=30)
    if out.returncode:
        raise AssertionError(out.stderr)
    return out.stdout


@unittest.skipUnless(NODE, "node not installed")
class TableTests(unittest.TestCase):
    def test_plain_text_unchanged(self):
        self.assertEqual(render("hello\nworld"), '<div class="txt">hello\nworld</div>')

    def test_table_between_prose(self):
        html = render("Here:\n\n| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\nDone.")
        self.assertIn('<div class="txt">Here:</div><div class="tbl"><table>', html)
        self.assertIn("<thead><tr><th>A</th><th>B</th></tr></thead>", html)
        self.assertIn("<tr><td>1</td><td>2</td></tr><tr><td>3</td><td>4</td></tr>", html)
        self.assertTrue(html.endswith('</table></div><div class="txt">Done.</div>'))

    def test_alignment(self):
        html = render("| l | c | r |\n|:--|:-:|--:|\n| a | b | c |")
        self.assertIn('<td style="text-align:left">a</td>', html)
        self.assertIn('<td style="text-align:center">b</td>', html)
        self.assertIn('<td style="text-align:right">c</td>', html)

    def test_without_outer_pipes(self):
        html = render("A | B\n--- | ---\n1 | 2")
        self.assertIn("<th>A</th><th>B</th>", html)
        self.assertIn("<td>1</td><td>2</td>", html)

    def test_escaped_pipe_and_inline(self):
        html = render("| k | v |\n|---|---|\n| `a\\|b` | **yes** |")
        self.assertIn("<td><code>a|b</code></td><td><b>yes</b></td>", html)

    def test_cells_are_escaped(self):
        html = render("| x |\n|---|\n| <script> |")
        self.assertIn("<td>&lt;script&gt;</td>", html)
        self.assertNotIn("<script>", html)

    def test_short_and_long_rows_fit_header(self):
        html = render("| a | b |\n|---|---|\n| 1 |\n| 1 | 2 | 3 |")
        self.assertIn("<tr><td>1</td><td></td></tr>", html)
        self.assertIn("<tr><td>1</td><td>2</td></tr>", html)

    def test_inside_fence_left_alone(self):
        text = "```\n| a | b |\n|---|---|\n| 1 | 2 |\n```"
        self.assertNotIn("<table>", render(text))

    def test_mismatched_delimiter_is_not_a_table(self):
        self.assertNotIn("<table>", render("| a | b |\n|---|\n| 1 | 2 |"))

    def test_pipe_in_prose_is_not_a_table(self):
        self.assertNotIn("<table>", render("run a | b\nthen c"))


@unittest.skipUnless(NODE, "node not installed")
class MarkdownTests(unittest.TestCase):
    def test_bold_italic_strike_in_prose(self):
        html = render("this is **bold**, *italic*, _also_ and ~~gone~~")
        self.assertEqual(html, '<div class="txt">this is <b>bold</b>, <i>italic</i>, '
                               '<i>also</i> and <s>gone</s></div>')

    def test_snake_case_and_arithmetic_are_not_emphasis(self):
        html = render("call my_func_name with 2 * 3 * 4 in some_file.py")
        self.assertNotIn("<i>", html)
        self.assertIn("my_func_name", html)

    def test_nothing_inside_inline_code_is_formatted(self):
        self.assertIn("<code>a **b** c</code>", render("run `a **b** c` now"))

    def test_headings(self):
        html = render("## Summary\ntext")
        self.assertIn('<div class="h h2">Summary</div>', html)
        self.assertIn('<div class="txt">text</div>', html)

    def test_bullet_list_with_inline_formatting(self):
        html = render("Changes:\n- **one**\n- two\n\nDone.")
        self.assertIn('<div class="md"><ul><li><b>one</b></li><li>two</li></ul></div>', html)
        self.assertTrue(html.endswith('<div class="txt">Done.</div>'))

    def test_numbered_list_keeps_its_start(self):
        self.assertIn('<ol start="3"><li>c</li><li>d</li></ol>', render("3. c\n4. d"))

    def test_nested_list(self):
        self.assertIn("<ul><li>a<ul><li>b</li></ul></li><li>c</li></ul>",
                      render("- a\n  - b\n- c"))

    def test_a_blank_line_between_items_keeps_one_list(self):
        self.assertEqual(render("- a\n\n- b").count("<ul>"), 1)

    def test_fenced_code_is_escaped_and_left_raw(self):
        self.assertEqual(render("```python\nx = **y** <z>\n```"),
                         '<pre class="code">x = **y** &lt;z&gt;</pre>')

    def test_unclosed_fence_runs_to_the_end(self):
        self.assertIn('<pre class="code">a\n- b</pre>', render("```\na\n- b"))

    def test_blockquote_and_rule(self):
        html = render("> quoted **x**\n\n---\nafter")
        self.assertIn('<blockquote><div class="txt">quoted <b>x</b></div></blockquote>', html)
        self.assertIn("<hr>", html)

    def test_links_only_for_http(self):
        html = render("[docs](https://example.com/a_b) and [x](javascript:alert(1))")
        self.assertIn('<a href="https://example.com/a_b" target="_blank" '
                      'rel="noopener">docs</a>', html)
        self.assertNotIn('href="javascript', html)

    def test_bare_url_is_linked_without_trailing_punctuation(self):
        self.assertIn('>https://github.com/a/b_c_d</a>.', render("see https://github.com/a/b_c_d."))

    def test_html_in_prose_is_escaped(self):
        html = render("**<img src=x onerror=alert(1)>**")
        self.assertIn("<b>&lt;img src=x onerror=alert(1)&gt;</b>", html)
        self.assertNotIn("<img", html)


@unittest.skipUnless(NODE, "node not installed")
class Drafts(unittest.TestCase):
    """The prompt box keeps one unsent draft per session."""

    def run_js(self, steps):
        start = ui.SCRIPT.index("// -- one unsent draft per session")
        body = ui.SCRIPT[start:ui.SCRIPT.index("let ticks = 0;")]
        js = ("const box = {value: ''}; const $ = () => box; let selected = null;\n"
              "const store = {}; const localStorage = {getItem: k => store[k] || null,"
              " setItem: (k, v) => { store[k] = v; }};\n" + body + "\n" + steps)
        out = subprocess.run([NODE, "-"], input=js, capture_output=True, text=True,
                             encoding="utf-8", timeout=30)
        if out.returncode:
            raise AssertionError(out.stderr)
        return json.loads(out.stdout)

    def test_switching_parks_and_restores(self):
        got = self.run_js(
            "const seen = [];"
            "selected = 'a'; syncDraft(); box.value = 'half a thought'; noteDraft();"
            "selected = 'b'; syncDraft(); seen.push(box.value); box.value = 'other'; noteDraft();"
            "selected = 'a'; syncDraft(); seen.push(box.value);"
            "selected = 'b'; syncDraft(); seen.push(box.value);"
            "process.stdout.write(JSON.stringify(seen));")
        self.assertEqual(got, ["", "half a thought", "other"])

    def test_sent_draft_is_gone_and_late_clear_spares_the_new_box(self):
        got = self.run_js(
            "selected = 'a'; syncDraft(); box.value = 'go'; noteDraft();"
            "selected = 'b'; syncDraft(); box.value = 'typing in b'; noteDraft();"
            "clearPrompt('a');"  # a's send finished after the switch
            "const inB = box.value; selected = 'a'; syncDraft();"
            "process.stdout.write(JSON.stringify([inB, box.value]));")
        self.assertEqual(got, ["typing in b", ""])


@unittest.skipUnless(NODE, "node not installed")
class SessionTree(unittest.TestCase):
    """A project with several sessions gathers them under one node."""

    def run_js(self, group, sessions, folded=(), waiting=(), then=None):
        s = ui.SCRIPT
        part = lambda a, b: s[s.index(a):s.index(b, s.index(a))]
        js = ("const store = {'imperatorium.folded': %s, 'imperatorium.waithidden': %s};\n"
              "const localStorage = {getItem: k => store[k] || null, setItem: (k, v) => { store[k] = v; }};\n"
              "const drawSessions = () => {};\n"
              % (json.dumps(json.dumps(list(folded))), json.dumps(json.dumps(list(waiting))))
              + part("const esc =", "\n\n") + "\n" + part("const folder =", "\n") + "\n"
              + part("const nameOf =", "\nfunction groupOf") + "\n"
              + part("function tree(", "\nfunction drawSessions") + "\n"
              "const stateOf = s => [s.attention ? 'd-attn' : 'd-idle', 'x'];\n"
              "const isUnread = s => !!s.unread;\n"
              "const tile = (s, short) => '[' + (short && s.short_name || nameOf(s)) + ']';\n"
              "const prefs = {group: %s};\n" % json.dumps(group)
              + (then or "process.stdout.write(tree(%s));" % json.dumps(sessions)))
        out = subprocess.run([NODE, "-"], input=js, capture_output=True, text=True,
                             encoding="utf-8", timeout=30)
        if out.returncode:
            raise AssertionError(out.stderr)
        return out.stdout

    SESSIONS = [
        {"name": "hai-proof-5b", "short_name": "5b", "cwd": "C:/src/hai-proof",
         "project": "hai-proof"},
        {"name": "strata-0b", "short_name": "0b", "cwd": "C:/src/strata", "project": "strata"},
        {"name": "fizzy-wondering-lark-c9", "display_name": "hai-proof-fizzy-wondering-lark",
         "short_name": "fizzy-wondering-lark",
         "cwd": "C:/src/hai-proof/.claude/worktrees/fizzy-wondering-lark",
         "project": "hai-proof", "worktree": "fizzy-wondering-lark", "attention": True},
    ]

    def test_siblings_nest_under_their_project_by_short_name(self):
        html = self.run_js("state", self.SESSIONS)
        self.assertIn('data-fam="hai-proof"', html)
        self.assertIn('<span class="car">▾</span><span>hai-proof</span>', html)
        self.assertIn('<div class="kids">[5b][fizzy-wondering-lark]</div>', html)
        self.assertTrue(html.endswith("[strata-0b]"))

    def test_one_dot_per_session_on_the_node(self):
        html = self.run_js("state", self.SESSIONS)
        dots = html[html.index('class="dots"'):html.index('<div class="kids">')]
        self.assertEqual(dots.count('class="dot '), 2)
        self.assertIn("d-attn", dots)

    def test_folded_node_hides_everything_but_its_dots(self):
        html = self.run_js("state", self.SESSIONS, folded=["hai-proof"])
        self.assertIn('class="fam shut"', html)
        self.assertNotIn("kids", html)
        self.assertNotIn("[fizzy-wondering-lark]", html)
        self.assertEqual(html.count('class="dot '), 2)

    def test_hiding_waiting_keeps_the_others(self):
        html = self.run_js("state", self.SESSIONS, waiting=["hai-proof"])
        self.assertIn('<div class="kids">[5b]</div>', html)
        self.assertIn("waiting hidden", html)
        self.assertEqual(html.count('class="dot '), 2)

    def test_folded_wins_over_waiting_hidden(self):
        html = self.run_js("state", self.SESSIONS, folded=["hai-proof"],
                           waiting=["hai-proof"])
        self.assertNotIn("kids", html)
        self.assertNotIn("waiting hidden", html)

    def clicks(self, script):
        return json.loads(self.run_js("state", [], then=(
            script + "; setTimeout(() => process.stdout.write(JSON.stringify("
            "[[...folded], [...waitHidden]])), DOUBLE_MS + 50);")))

    def test_a_single_click_folds_once_the_double_click_window_passes(self):
        self.assertEqual(self.clicks("famClicked('a')"), [["a"], []])

    def test_a_double_click_hides_waiting_and_does_not_fold(self):
        self.assertEqual(self.clicks("famClicked('a'); famClicked('a')"), [[], ["a"]])

    def test_unfolding_shows_the_waiting_ones_too(self):
        got = json.loads(self.run_js("state", [], folded=["a"], waiting=["a", "b"], then=(
            "famClicked('a'); setTimeout(() => process.stdout.write(JSON.stringify("
            "[[...folded], [...waitHidden], JSON.parse(store['imperatorium.waithidden'])])), DOUBLE_MS + 50);")))
        self.assertEqual(got, [[], ["b"], ["b"]])

    def test_folding_leaves_the_waiting_flag_for_the_unfold_to_clear(self):
        got = self.clicks("waitHidden.add('a'); famClicked('a')")
        self.assertEqual(got, [["a"], ["a"]])

    def test_clicks_on_two_nodes_fold_both(self):
        self.assertEqual(sorted(self.clicks("famClicked('a'); famClicked('b')")[0]),
                         ["a", "b"])

    def test_a_lone_session_is_a_plain_tile(self):
        self.assertEqual(self.run_js("state", self.SESSIONS[1:2]), "[strata-0b]")

    def test_grouped_by_folder_a_lone_session_still_has_no_node(self):
        html = self.run_js("folder", self.SESSIONS)
        self.assertIn('<div class="kids">[5b][fizzy-wondering-lark]</div>', html)
        self.assertNotIn('data-fam="strata"', html)
        self.assertTrue(html.endswith("[strata-0b]"))

    def test_hover_opens_a_tile_and_carries_its_pin_and_close(self):
        self.assertIn(".s:not(.on):not(:hover) :is(.m,.stl,.x){display:none}", ui.CSS)
        self.assertIn(".s:is(.on,:hover) .why{display:none}", ui.CSS)
        self.assertNotIn(".s:hover .acts", ui.CSS)
        self.assertIn('<span class="acts">${acts}</span></div>`;', ui.SCRIPT)

    def test_tile_x_closes_a_running_local_session_rather_than_archiving(self):
        for needle in ('data-close="${esc(key(s))}"', "s.pid && !s.remote",
                       'post("/api/close"', "closeSession(el.dataset.close)",
                       "famClicked(el.dataset.fam)"):
            self.assertIn(needle, ui.SCRIPT)
        self.assertNotIn('archived ? "↩" : "✕"', ui.SCRIPT)

    def test_an_archived_tile_with_nothing_running_can_be_dismissed(self):
        for needle in ('data-dismiss="${esc(key(s))}"', "!s.pid && !s.remote",
                       'post("/api/dismiss"', "dismissSession(el.dataset.dismiss)"):
            self.assertIn(needle, ui.SCRIPT)

    def test_notification_switches_are_in_settings_not_insights(self):
        self.assertNotIn("id=notif", ui.render_shell(""))
        self.assertNotIn('post("/api/notify"', ui.SCRIPT)
        for needle in ('sec.id === "notify" ? notifyExtrasHtml(d)', 'id="desk"',
                       "Notification.requestPermission()", '"imperatorium.desknotify"'):
            self.assertIn(needle, ui.SETTINGS_SCRIPT)
        # the main window picks up the switch flipped in the Settings window
        self.assertIn('e.key === "imperatorium.desknotify"', ui.SCRIPT)

    def test_vs_code_tiles_carry_their_own_icon(self):
        self.assertIn('s.desktop_app ? APP_ICON : s.ide === "vscode" ? IDE_ICON : ""', ui.SCRIPT)

    def test_every_page_reloads_for_a_new_dispatcher(self):
        self.assertIn("async function checkBoot(onNew)", ui.SHARED)
        for script in (ui.SCRIPT, ui.MATRIX_SCRIPT, ui.SETTINGS_SCRIPT):
            self.assertIn("checkBoot(", script.replace(ui.SHARED, ""))
        # the window keeps its session; Settings never drops unsaved edits
        self.assertIn('"?session=" + encodeURIComponent(selected)', ui.SCRIPT)
        self.assertIn("Object.keys(changes()).length) say(", ui.SETTINGS_SCRIPT)

    def test_worktree_tiles_carry_the_icon(self):
        self.assertIn("s.worktree ? WT_ICON", ui.SCRIPT)

@unittest.skipUnless(NODE, "node not installed")
class Shape(unittest.TestCase):
    """Each kind of text shows in full, folded to one line, or not at all."""

    TURNS = [
        {"speaker": "you", "blocks": [{"kind": "text", "text": "do the thing"}]},
        {"speaker": "claude", "blocks": [
            {"kind": "thinking", "text": "\n  first thought\nsecond"},
            {"kind": "text", "text": "on it"},
            {"kind": "tool", "name": "Bash", "summary": "ls -la", "id": "t1"},
            {"kind": "result", "text": "boom\nmore", "is_error": True, "id": "t1"}]},
        {"speaker": "system", "note": "task finished",
         "blocks": [{"kind": "text", "text": "build passed"}]},
    ]

    def run_js(self, shape, opened=()):
        start = ui.SCRIPT.index("const esc =")
        esc = ui.SCRIPT[start:ui.SCRIPT.index("\n\n", start)]
        a = ui.SCRIPT.index("function renderBlock(b)")
        body = ui.SCRIPT[a:ui.SCRIPT.index("function drawShape")]
        js = ("const renderText = t => '[' + t + ']';\n"
              "const localStorage = {getItem: () => %s};\n" % json.dumps(json.dumps(shape))
              + esc + "\n" + body +
              "\nconst opened = new Set(%s);" % json.dumps(list(opened)) +
              "\nprocess.stdout.write(JSON.stringify(%s.map((it, i) => shapeBlocks(it, i, opened))));"
              % json.dumps(self.TURNS))
        out = subprocess.run([NODE, "-"], input=js, capture_output=True, text=True,
                             encoding="utf-8", timeout=30)
        if out.returncode:
            raise AssertionError(out.stderr)
        return json.loads(out.stdout)

    def test_full_by_default_is_the_old_rendering(self):
        you, claude, note = self.run_js({})
        self.assertEqual(you, "[do the thing]")
        self.assertIn('<div class="think">', claude)
        self.assertIn("[on it]", claude)
        self.assertIn("<b>Bash</b> ls -la", claude)
        self.assertIn('class="res err"', claude)
        self.assertNotIn("<details", claude)
        self.assertEqual(note, "[build passed]")

    def test_hidden_drops_only_its_kind(self):
        you, claude, note = self.run_js({"thinking": "hidden", "result": "hidden",
                                         "system": "hidden"})
        self.assertEqual(you, "[do the thing]")
        self.assertNotIn("think", claude)
        self.assertNotIn("res", claude)
        self.assertIn("[on it]", claude)
        self.assertEqual(note, "")

    def test_compact_is_one_line_that_opens(self):
        you, claude, _ = self.run_js({"thinking": "compact", "tool": "compact",
                                      "result": "compact", "prompt": "compact"},
                                     opened=["1.2"])
        self.assertIn('<details class="cmp" data-k="0.0"><summary><span class="ln">do the thing',
                      you)
        self.assertIn('<span class="k">thinking</span><span class="ln">first thought</span>',
                      claude)
        self.assertIn('data-k="1.2" open><summary><span class="k">Bash</span><span class="ln">ls -la',
                      claude)
        self.assertIn('<span class="k err">error</span><span class="ln">boom</span>', claude)
        # Replies were left at Full.
        self.assertIn("</details>[on it]<details", claude)

    def test_unknown_mode_falls_back_to_full(self):
        you, _, _ = self.run_js({"prompt": "sideways"})
        self.assertEqual(you, "[do the thing]")


@unittest.skipUnless(NODE, "node not installed")
@unittest.skipUnless(NODE, "node is not installed")
class WorkAtAGlance(unittest.TestCase):
    """The tile's cache countdown, its signals, and what the open tile adds."""

    def run_tile_js(self, call):
        start = ui.SCRIPT.index("const esc =")
        esc = ui.SCRIPT[start:ui.SCRIPT.index("\n\n", start)]
        kfmt = ui.SCRIPT[ui.SCRIPT.index("function kfmt"):ui.SCRIPT.index("function span")]
        span = ui.SCRIPT[ui.SCRIPT.index("function span"):]
        span = span[:span.index("\n}\n") + 3]
        usd = ui.SCRIPT[ui.SCRIPT.index("const usd ="):]
        usd = usd[:usd.index("\n") + 1]
        w = ui.SCRIPT.index("const CACHE_SOON")
        js = (esc + "\n" + kfmt + span + usd +
              "const stateOf = s => [s.status === 'busy' ? 'd-busy' : 'd-idle', s.status];\n"
              "const isUnread = s => !!s.unread; const heavyWarnings = () => [];\n"
              "const prBadge = pr => '[PR]'; const age = t => 'AGE';\n" +
              ui.SCRIPT[w:ui.SCRIPT.index("function tile(")] +
              "\nprocess.stdout.write(JSON.stringify(" + call + "));")
        out = subprocess.run([NODE, "-"], input=js, capture_output=True, text=True,
                             encoding="utf-8", timeout=30)
        if out.returncode:
            raise AssertionError(out.stderr)
        return json.loads(out.stdout)

    def parts(self, work, now=1000, all=False):
        return self.run_tile_js("workParts(%s, %d, %s).map(p => p.text)"
                                % (json.dumps(work), now, json.dumps(all)))

    def test_a_closed_tile_counts_down_only_the_last_ten_minutes(self):
        far = {"cache_expires_at": 1000 + 58 * 60 - 10}
        self.assertEqual(self.parts(far), [])
        self.assertEqual(self.parts(far, all=True), ["-58"])
        self.assertEqual(self.parts({"cache_expires_at": 1000 + 4 * 60 - 10}), ["-4"])

    def test_minutes_round_up_and_an_expired_cache_shows_nothing(self):
        self.assertEqual(self.parts({"cache_expires_at": 1030}), ["-1"])
        self.assertEqual(self.parts({"cache_expires_at": 900}, all=True), [])
        self.assertEqual(self.parts(None), [])

    def test_a_scheduled_turn_says_when_on_any_tile(self):
        w = {"alive": True, "wakes_at": 1000 + 20 * 60, "wakeups": 1}
        self.assertEqual(self.parts(w), ["wakes 20m"])
        self.assertEqual(self.parts(dict(w, wakes_at=900)), [])
        self.assertEqual(self.parts(dict(w, alive=False)), [])

    def test_the_four_counts_are_gone_from_the_tile_and_status_line(self):
        self.assertIn('workHtml(s.work, now, "badge work", open)', ui.SCRIPT)
        self.assertNotIn('n.join("/")', ui.SCRIPT)

    def details(self, row, now=10000, spend=None):
        return self.run_tile_js("tileDetails(%s, %d, %s).map(p => p.text)"
                                % (json.dumps(row), now, json.dumps(spend)))

    def test_the_open_tile_says_idle_time_context_cost_and_whats_running(self):
        got = self.details({"status": "idle", "status_changed_at": 10000 - 720,
                            "usage": {"context_pct": 45.2},
                            "work": {"context_tokens": 230000, "alive": True,
                                     "agents": 1, "shells": 2, "monitors": 1, "processes": 2}},
                           spend={"cost_usd": 4.1})
        self.assertEqual(got, ["idle 12m", "ctx 230k (45%)", "≈$4.10", "3 running"])
        waiting = self.details({"status": "idle", "attention": True, "status_changed_at": 10000 - 720})
        self.assertEqual(waiting, ["idle 12m"])

    def test_a_working_session_says_how_long_and_nothing_running_is_left_out(self):
        got = self.details({"status": "busy", "status_changed_at": 10000 - 180,
                            "work": {"alive": True, "agents": 0, "processes": 0}})
        self.assertEqual(got, ["working 3m"])

    def signals(self, row, now=1000, open=False):
        return self.run_tile_js("tileSignals(%s, %d, %s)" % (json.dumps(row), now, json.dumps(open)))

    def test_closed_signals_appear_only_when_out_of_the_ordinary(self):
        self.assertEqual(self.signals({"usage": {"context_pct": 40},
                                       "work": {"cache_expires_at": 3000},
                                       "pr": {"state": "open", "checks": "passing"}}), "")
        got = self.signals({"unread": True, "queued": 2, "usage": {"context_pct": 91},
                            "work": {"cache_expires_at": 1200},
                            "attention": True, "attention_since": 40,
                            "pr": {"state": "open", "checks": "failing"}})
        order = [got.index(x) for x in (">new<", ">-4<", ">ctx 91%<", ">2 queued<", "[PR]")]
        self.assertEqual(order, sorted(order))
        self.assertNotIn('class="since"', got)   # the open tile's idle time says it

    def test_the_open_tile_keeps_context_and_pr_for_its_details_line(self):
        got = self.signals({"usage": {"context_pct": 91}, "work": {"cache_expires_at": 3000},
                            "pr": {"state": "open", "checks": "failing"}}, open=True)
        self.assertNotIn("ctx", got)
        self.assertNotIn("[PR]", got)
        self.assertIn(">-34<", got)


@unittest.skipUnless(NODE, "node not installed")
class StatusPanel(unittest.TestCase):
    """The panel under the send buttons, rendered from a row and its status."""

    def run_js(self, row, status, folded=False):
        start = ui.SCRIPT.index("const esc =")
        esc = ui.SCRIPT[start:ui.SCRIPT.index("\n\n", start)]
        a = ui.SCRIPT.index("function kfmt(")
        body = ui.SCRIPT[a:ui.SCRIPT.index("function drawStatus")]
        w = ui.SCRIPT.index("const CACHE_SOON")
        work = ui.SCRIPT[w:ui.SCRIPT.index("function tile(")]
        js = (esc + "\nconst age = t => 'AGE';\nDate.now = () => 1000 * 1000;\n"
              "const stateOf = s => ['d-busy', s.label];\n" + work + body +
              "\nprocess.stdout.write(statusHtml(%s, %s, 1000, %s));"
              % (json.dumps(row), json.dumps(status), json.dumps(folded)))
        out = subprocess.run([NODE, "-"], input=js, capture_output=True, text=True,
                             encoding="utf-8", timeout=30)
        if out.returncode:
            raise AssertionError(out.stderr)
        return out.stdout

    ROW = {"label": "waiting on a background task", "queued": 2,
           "usage": {"context_pct": 17.4, "context_size": 1000000, "cost_usd": 3.456,
                     "model": "Opus 5.5", "lines_added": 12, "lines_removed": 3}}
    STATUS = {"ok": True, "shell_processes": 1, "stat": {
        "agents": [], "monitors": [{"id": "m", "label": "watch <log>", "started": 900,
                                    "expires": 1600}],
        "shells": [{"id": "b", "label": "make test"}],
        "tokens": {"input": 10, "output": 2500, "cache_read": 1200000, "cache_write": 40000},
        "cache": {"ttl": 3600, "expires_at": 1300, "warm": True, "hit": 0.97},
        "context_tokens": 174000, "prompts": 7, "turns": 6, "permission_mode": "auto",
        "compactions": 2, "branch": "main", "version": "2.1.296"}}

    def test_summary_line(self):
        html = self.run_js(self.ROW, self.STATUS)
        head = html[:html.index('<div class="bd">')]
        for want in ("waiting on a background task", "1 shell", "1 monitor",
                     "cache -5m", "$3.46"):
            self.assertIn(want, head)
        self.assertNotIn("subagent", head)

    def test_the_summary_line_has_no_slash_counts(self):
        row = dict(self.ROW, work={"cache_expires_at": 1000 + 300, "alive": True,
                                   "agents": 1, "shells": 2, "monitors": 1, "processes": 0})
        html = self.run_js(row, self.STATUS)
        head = html[:html.index('<div class="bd">')]
        self.assertIn("cache -5m · $3.46</span>", head)
        self.assertNotIn("1/2/1/0", head)

    def test_detail_cells(self):
        html = self.run_js(self.ROW, self.STATUS)
        for want in ('<b>processes</b><span class="hot">1<',"warm · 5m left · 97% hit", "17% · 174k / 1.0M",
                     "1.2M in · 2.5k out", "1.2M read · 40k written", "7 · 6 turns",
                     "+12 −3", ">auto<", ">main<", ">2<", "ends in 10m", "watch &lt;log&gt;"):
            self.assertIn(want, html)

    def test_cold_cache_and_no_transcript(self):
        status = json.loads(json.dumps(self.STATUS))
        status["stat"]["cache"]["expires_at"] = 400
        self.assertIn("cold · 10m ago", self.run_js(self.ROW, status))
        bare = self.run_js({"label": "idle"}, {"ok": True, "note": "remote sessions report less"})
        self.assertNotIn("subagents", bare)
        self.assertIn("remote sessions report less", bare)

    def test_shell_has_the_panel_under_the_buttons(self):
        html = ui.render_shell()
        self.assertLess(html.index("id=bar"), html.index("id=status"))
        self.assertLess(html.index("id=status"), html.index("id=queued"))


@unittest.skipUnless(NODE, "node is not installed")
class LogWindow(unittest.TestCase):
    """A long chat draws its newest turns first; recent chats stay in the page."""

    def run_js(self, expr):
        a = ui.SCRIPT.index("const LOG_WINDOW")
        body = ui.SCRIPT[a:ui.SCRIPT.index("function setLog")]
        js = body + "\nprocess.stdout.write(JSON.stringify(%s));" % expr
        out = subprocess.run([NODE, "-"], input=js, capture_output=True, text=True, encoding="utf-8",
                             timeout=30)
        if out.returncode:
            raise AssertionError(out.stderr)
        return json.loads(out.stdout)

    def test_only_the_newest_turns_are_drawn_until_asked(self):
        self.assertEqual(self.run_js("[logStart(387, LOG_WINDOW), logStart(20, LOG_WINDOW),"
                                     " logStart(387, LOG_WINDOW + LOG_STEP), logStart(387, 387)]"),
                         [327, 0, 127, 0])

    def test_recent_conversations_are_kept_newest_last(self):
        got = self.run_js("(() => { for (let i = 0; i < CONV_KEEP + 2; i++) rememberConv('s' + i, [], 'g' + i);"
                          " rememberConv('s3', [1], 'new');"
                          " return [convCache.size, [...convCache.keys()][0],"
                          " [...convCache.keys()].pop(), convCache.get('s3').sig]; })()")
        self.assertEqual(got, [8, "s2", "s3", "new"])

    def test_the_window_wires_it_up(self):
        for needle in ("showCached()", "data-earlier", "want !== selected",
                       "/api/voice/warm"):
            self.assertIn(needle, ui.SCRIPT)


class Controls(unittest.TestCase):
    @unittest.skipUnless(NODE, "node not installed")
    def test_the_whole_script_parses(self):
        # The other tests slice pieces out; a typo anywhere else would only
        # show as a blank window.
        out = subprocess.run([NODE, "-e", "new Function(require('fs').readFileSync(0, 'utf8'))"],
                             input=ui.SCRIPT, capture_output=True, text=True,
                             encoding="utf-8", timeout=30)
        self.assertEqual(out.returncode, 0, out.stderr)

    def test_nothing_is_set_below_12px_but_the_fold_carets(self):
        import re
        small = [m.group(0) for m in re.finditer(r"[^;{}]*font(?:-size)?:\s*(\d+(?:\.\d+)?)px[^;}]*", ui.CSS)
                 if float(m.group(1)) < 12]
        self.assertEqual(len(small), 2, small)
        self.assertTrue(all("10px" in x for x in small))

    def test_no_invalid_font_shorthand(self):
        # `font:13px inherit` is not valid CSS; the browser drops the whole
        # declaration, and buttons and the prompt box fell back to its fonts.
        import re
        self.assertEqual(re.findall(r"font:[^;}]*\binherit", ui.CSS), [])

    def test_every_face_named_in_the_css_is_bundled(self):
        import re
        for name in re.findall(r"url\(/fonts/([^)]+)\)", ui.CSS):
            self.assertEqual(ui.font(name)[:4], b"wOF2", name)
        self.assertIsNone(ui.font("../ui.py"))
        self.assertNotIn("Consolas,ui-monospace", ui.CSS.split("--mono:")[1].split(";", 1)[1])

    def test_each_state_has_its_own_shape(self):
        for cls, shape in ((".d-idle", "box-shadow"), (".d-attn", "rotate(45deg)"),
                           (".d-done", "height:2px")):
            rule = ui.CSS[ui.CSS.index(cls + "{"):]
            self.assertIn(shape, rule[:rule.index("}")], cls)
        self.assertIn(".d-fail::after{transform:rotate(-45deg)}", ui.CSS)

    def test_shell_has_the_new_controls(self):
        html = ui.render_shell()
        for needle in ("id=curbtn", "Show Current Prompt", "id=jprev", "id=jnext",
                       "id=current", "id=shape"):
            self.assertIn(needle, html)

    def test_session_controls_share_the_header_row(self):
        # Peer request 2026-10-18: the tabs, the launch actions, the view
        # options, find and prompt jump all on the session header's row, in
        # that order, and nothing left between the chat and the prompt box.
        # The buttons that stay (Show Window) sit between find's arrows and
        # prompt jump's, to keep the two pairs apart.
        html = ui.render_shell()
        head = html[html.index("id=headrow"):html.index("id=sessionpane")]
        order = [head.index(n) for n in ("id=headtext", "id=tabpick", "id=launch",
                                         "id=viewmenu", "id=findbar", "id=acts", "id=jump")]
        self.assertEqual(order, sorted(order))
        launch = head[head.index("id=launch"):head.index("id=viewmenu")]
        for act in ("data-act=new ", "data-act=new-close", "data-act=fork-window",
                    "data-act=fork-background"):
            self.assertIn(act, launch)
        view = head[head.index("id=viewmenu"):head.index("id=findbar")]
        for needle in ("data-act=copy", "id=curbtn", "id=shape"):
            self.assertIn(needle, view)
        self.assertNotIn("id=strip", html)
        self.assertNotIn("id=tabs", html)
        self.assertNotIn("#strip", ui.SCRIPT)
        self.assertNotIn("#tabs", ui.SCRIPT)


@unittest.skipUnless(NODE, "node not installed")
class FollowNewSession(unittest.TestCase):
    """New selects the session it launched once that shows up on the board."""

    def run_js(self, steps):
        start = ui.SCRIPT.index("let following = null;")
        body = ui.SCRIPT[start:ui.SCRIPT.index("// -- queue", start)]
        js = ("let sessions = [], selected = 'old'; const key = s => s.sessionId;"
              " const calls = []; const showCached = () => calls.push('paint');"
              " const drawSessions = () => {}; const refreshTab = () => {};"
              " const loadStatus = () => {}; const loadProcs = () => {}; let view = 'session'; const setView = v => view = v;\n"
              + body + "\n" + steps)
        out = subprocess.run([NODE, "-"], input=js, capture_output=True, text=True,
                             encoding="utf-8", timeout=30)
        if out.returncode:
            raise AssertionError(out.stderr)
        return json.loads(out.stdout)

    def test_selected_once_it_appears(self):
        got = self.run_js(
            "follow('new'); sessions = [{sessionId: 'old'}]; catchFollowed();"
            " const before = selected; sessions.push({sessionId: 'new'}); catchFollowed();"
            " const after = selected; selected = 'old'; catchFollowed();"
            " process.stdout.write(JSON.stringify([before, after, selected, calls.length]));")
        self.assertEqual(got, ["old", "new", "old", 1])

    def test_given_up_after_the_deadline(self):
        got = self.run_js(
            "follow('new'); following.until = Date.now() - 1;"
            " sessions = [{sessionId: 'new'}]; catchFollowed();"
            " process.stdout.write(JSON.stringify([selected, following]));")
        self.assertEqual(got, ["old", None])

    def test_the_header_offers_the_buttons(self):
        for needle in ('data-act="show"', ">Show Window<", "follow(r.session_id)",
                       "catchFollowed();"):
            self.assertIn(needle, ui.SCRIPT)
        # New+close lives in the header's New / Fork menu, in the shell.
        for needle in ("data-act=new-close", ">New+close<"):
            self.assertIn(needle, ui.render_shell())


def matrix_js(tail):
    """Run the chat matrix's layout and tile code in node with `tail` after it."""
    script = ui.MATRIX_SCRIPT
    start = script.index("// Tiles are 1 wide")
    end = script.index("let data =")
    js = ui.SHARED + script[start:end] + tail
    out = subprocess.run([NODE, "-"], input=js, capture_output=True, text=True,
                         encoding="utf-8", timeout=30)
    if out.returncode:
        raise AssertionError(out.stderr)
    return json.loads(out.stdout)


class ChatMatrix(unittest.TestCase):
    @unittest.skipUnless(NODE, "node not installed")
    def test_the_matrix_script_parses(self):
        out = subprocess.run([NODE, "-e", "new Function(require('fs').readFileSync(0, 'utf8'))"],
                             input=ui.MATRIX_SCRIPT, capture_output=True, text=True,
                             encoding="utf-8", timeout=30)
        self.assertEqual(out.returncode, 0, out.stderr)

    def layouts(self, cases):
        calls = ",".join("layout(%d,%d,%d)" % c for c in cases)
        return matrix_js("process.stdout.write(JSON.stringify([%s]));" % calls)

    @unittest.skipUnless(NODE, "node not installed")
    def test_tiles_keep_the_ratio_and_fit(self):
        for (n, W, H), L in zip([(1, 1600, 900), (4, 1600, 900), (7, 1600, 900), (12, 1000, 1400),
                                 (5, 400, 1600)],
                                self.layouts([(1, 1600, 900), (4, 1600, 900), (7, 1600, 900),
                                              (12, 1000, 1400), (5, 400, 1600)])):
            self.assertAlmostEqual(L["h"] / L["w"], 1.5)
            self.assertLessEqual(L["cols"] * L["w"] + 8 * (L["cols"] - 1), W + 0.01)
            self.assertLessEqual(L["rows"] * L["h"] + 8 * (L["rows"] - 1), H + 0.01)
            self.assertGreaterEqual(L["cols"] * L["rows"], n)
            # Spare cells all fit in the last row.
            self.assertLess(L["spare"], L["cols"])

    @unittest.skipUnless(NODE, "node not installed")
    def test_the_layout_is_the_biggest_possible(self):
        # Brute force: no other column count gives a bigger tile.
        for n in range(1, 17):
            for W, H in ((1600, 900), (900, 1600), (1200, 1200)):
                L = self.layouts([(n, W, H)])[0]
                for cols in range(1, n + 1):
                    rows = -(-n // cols)
                    w = min((W - 8 * (cols - 1)) / cols, (H - 8 * (rows - 1)) / rows / 1.5)
                    self.assertLessEqual(w, L["w"] + 0.5, (n, W, H, cols))

    @unittest.skipUnless(NODE, "node not installed")
    def test_a_wide_window_puts_four_in_a_row(self):
        L = self.layouts([(4, 1600, 900)])[0]
        self.assertEqual((L["cols"], L["rows"], L["spare"]), (4, 1, 0))
        L = self.layouts([(5, 1600, 900)])[0]
        self.assertEqual((L["cols"], L["rows"], L["spare"]), (5, 1, 0))
        L = self.layouts([(7, 1600, 900)])[0]
        self.assertEqual(L["cols"] * L["rows"] - 7, L["spare"])

    @unittest.skipUnless(NODE, "node not installed")
    def test_a_tile_shows_replies_then_the_step_in_flight(self):
        html = matrix_js(
            "process.stdout.write(JSON.stringify(tileBody({replies:[{text:'a',turn:1},"
            "{text:'b',turn:2}],current:{kind:'tool',name:'Bash',summary:'<x>',more:2}})));")
        self.assertIn('<div class="rp"><div class="txt">a</div></div>', html)
        self.assertIn('<div class="rp newturn">', html)
        self.assertIn("<b>Bash</b>", html)
        self.assertIn("&lt;x&gt;", html)
        self.assertIn("+2", html)
        self.assertLess(html.index(">b<"), html.index("Bash"))

    @unittest.skipUnless(NODE, "node not installed")
    def test_a_working_session_says_what_it_is_waiting_on(self):
        got = matrix_js(
            "const w = {agents: 1, shells: 2, monitors: 0, processes: 1};"
            "process.stdout.write(JSON.stringify(["
            " tileBody({status: 'busy', replies: [], work: w}),"
            " tileBody({status: 'busy', replies: [], work: {shells: 0, processes: 0}}),"
            " tileBody({status: 'idle', replies: [], work: w}),"
            " tileBody({status: 'idle', background_tasks: 1, replies: [], work: {shells: 1}}),"
            " tileBody({status: 'busy', replies: [], work: w, current: {kind: 'tool', name: 'Bash'}}),"
            "]));")
        busy, nothing, idle, background, tool = got
        self.assertIn('<div class="cur wait"', busy)
        self.assertIn('<span class="spin"></span>', busy)
        self.assertIn("waiting on 1 subagent · 2 background shells · 1 process", busy)
        self.assertNotIn("waiting on", nothing)
        self.assertNotIn("waiting on", idle)
        self.assertIn("waiting on 1 background shell", background)
        self.assertNotIn("waiting on", tool)

    @unittest.skipUnless(NODE, "node not installed")
    def test_a_session_in_a_turn_with_nothing_in_flight_says_it_is_working(self):
        # Peer request 2026-10-18: a busy tile between steps showed nothing
        # under its replies, so only the header's dot said it was working.
        got = matrix_js(
            "process.stdout.write(JSON.stringify(["
            " tileBody({status: 'busy', replies: [], work: {}}),"
            " tileBody({status: 'idle', state: 'working', replies: [], work: {}}),"
            " tileBody({status: 'busy', replies: [], work: {shells: 1}}),"
            " tileBody({status: 'busy', replies: [], work: {}, current: {kind: 'thinking', text: 'x'}}),"
            " tileBody({status: 'idle', background_tasks: 1, replies: [], work: {}}),"
            " tileBody({status: 'idle', replies: [], work: {}}),"
            " tileBody({status: 'busy', attention: true, replies: [], work: {}}),"
            "]));")
        busy, working, shell, thinking, background, idle, attn = got
        self.assertIn('<span class="spin"></span><span class="what">Agent working</span>', busy)
        self.assertIn("Agent working", working)
        for other in (shell, thinking, background, idle, attn):
            self.assertNotIn("Agent working", other)
        self.assertIn("waiting on 1 background shell", shell)

    def test_a_working_tiles_dot_pulses(self):
        self.assertIn(".th .dot.d-busy{animation:pulse", ui.MATRIX_CSS)
        self.assertIn(".th .dot.d-busy{animation:none}", ui.MATRIX_CSS)

    @unittest.skipUnless(NODE, "node not installed")
    def test_the_status_tile_shows_spend(self):
        html = matrix_js(
            "process.stdout.write(JSON.stringify(statusTile({cost_usd:12.345,costed:2,sessions:3,"
            "total_tokens:2500000,tokens:{input:1000,output:2000,cache_read:2400000,cache_write:97000},"
            "each:[{name:'a',cost_usd:12.345,tokens:2500000}],limits:{five_hour:{used:42}}})));")
        for needle in ("$12.35", "cost known for 2", "2.5M tokens", "5-hour", "42%", "By session"):
            self.assertIn(needle, html)

    def test_the_button_leads_the_sidebar_header(self):
        # Peer request 2026-10-18: the SESSIONS label went; the list speaks
        # for itself, and the header starts with its buttons: Settings, then
        # the Matrix Button (peer request 2026-10-18).
        shell = ui.render_shell()
        self.assertIn("<h1><span class=ttl><button id=setbtn", shell)
        self.assertNotIn("sessions <", shell[shell.index("<h1>"):shell.index("id=matrixbtn")])
        button = shell.index("id=matrixbtn")
        self.assertLess(shell.index("<h1>"), button)
        self.assertLess(button, shell.index("id=new"))
        self.assertIn('aria-label="Chat Matrix"', shell)
        self.assertIn("<svg", shell[button:shell.index("id=new")])
        for needle in ('window.open("/matrix", MATRIX', 'matrixChannel.postMessage("close")',
                       "$(\"#matrixbtn\").onclick = toggleMatrix"):
            self.assertIn(needle, ui.SCRIPT)

    def test_the_page_answers_close_and_says_it_is_up(self):
        for needle in ('"imperatorium-matrix"', 'e.data === "close") window.close()',
                       'postMessage("open")', 'postMessage("closed")'):
            self.assertIn(needle, ui.MATRIX_SCRIPT)
        self.assertIn("<title>Chat Matrix</title>", ui.render_matrix())

    def test_tiles_stretch_to_fill_once_the_grid_is_chosen(self):
        script = ui.MATRIX_SCRIPT
        self.assertIn("const cw = Math.max(0, (W - GAP * (L.cols - 1)) / L.cols);", script)
        self.assertIn("const rh = Math.max(0, (H - GAP * (L.rows - 1)) / L.rows);", script)
        self.assertIn("gridTemplateColumns: `repeat(${L.cols}, ${cw}px)`, gridAutoRows: `${rh}px`",
                      script)

    def test_nothing_in_the_matrix_scrolls(self):
        import re
        self.assertEqual(re.findall(r"overflow(?:-[xy])?:\s*(?:auto|scroll)", ui.MATRIX_CSS), [])
        self.assertNotIn("scrollTop", ui.MATRIX_SCRIPT)

    def test_the_attention_ring_is_outside_the_tile(self):
        # Inset, the header and the tile's contents painted over parts of it.
        self.assertNotIn("inset 0 0 0", ui.MATRIX_CSS)
        self.assertIn(".tile.attn{border-color:var(--warn);box-shadow:0 0 0 1px var(--warn)}",
                      ui.MATRIX_CSS)

    def test_a_double_click_shows_the_session_in_the_main_window(self):
        for needle in ('addEventListener("dblclick"', "channel.postMessage({select: sid})",
                       "window.opener.focus()", 'window.open("/?session=" + encodeURIComponent(sid)',
                       "el.dataset.sid = t.sessionId"):
            self.assertIn(needle, ui.MATRIX_SCRIPT)
        for needle in ("matrixChannel.postMessage({selected: m.select})",
                       'new URLSearchParams(location.search).get("session")',
                       'window.name = "imperatorium-main"'):
            self.assertIn(needle, ui.SCRIPT)

    @unittest.skipUnless(NODE, "node not installed")
    def test_the_main_window_selects_what_the_matrix_asks_for(self):
        start = ui.SCRIPT.index("if (matrixChannel) matrixChannel.onmessage")
        end = ui.SCRIPT.index("function toggleMatrix")
        js = ("let sent = [], picked = [], followed = [], sessions = [{sessionId: 'a'}], matrixSeen = 0;"
              "const key = s => s.sessionId, select = id => picked.push(id),"
              " follow = id => followed.push(id), showMatrixButton = () => {};"
              "const window = {focus(){}}, Date = {now: () => 5};"
              "const matrixChannel = {postMessage: m => sent.push(m)};"
              + ui.SCRIPT[start:end] +
              "matrixChannel.onmessage({data: {select: 'a'}});"
              "matrixChannel.onmessage({data: {select: 'later'}});"
              "matrixChannel.onmessage({data: 'open'});"
              "process.stdout.write(JSON.stringify([sent, picked, followed, matrixSeen]));")
        out = subprocess.run([NODE, "-"], input=js, capture_output=True, text=True,
                             encoding="utf-8", timeout=30)
        self.assertEqual(out.returncode, 0, out.stderr)
        sent, picked, followed, seen = json.loads(out.stdout)
        self.assertEqual(sent, [{"selected": "a"}, {"selected": "later"}])
        self.assertEqual((picked, followed, seen), (["a"], ["later"], 5))

    def test_no_custom_property_refers_to_itself(self):
        # `--mono:...,var(--mono)` is a cycle; the browser makes the property
        # invalid and every var(--mono) with it.
        import re
        for name, value in re.findall(r"(--[\w-]+)\s*:([^;}]*)", ui.CSS + ui.MATRIX_CSS):
            self.assertNotIn("var(%s)" % name, value, name)


def glance_js(tail):
    """The newer window helpers - unread, attention, keys, warnings, find,
    diff comments, cost chart - run in node with `tail` after them."""
    a = ui.SCRIPT.index("// -- what changed since you last looked")
    body = ui.SCRIPT[a:ui.SCRIPT.index("let ticks = 0;")]
    kfmt = ui.SCRIPT[ui.SCRIPT.index("function kfmt"):ui.SCRIPT.index("function span")]
    js = (ui.SHARED + "const key = s => s.sessionId || s.id; const nameOf = s => s.name || s.id;"
          " let selected = null;\n" + kfmt + body
          + "\nprocess.stdout.write(JSON.stringify(" + tail + "));")
    out = subprocess.run([NODE, "-"], input=js, capture_output=True, text=True,
                         encoding="utf-8", timeout=30)
    if out.returncode:
        raise AssertionError(out.stderr)
    return json.loads(out.stdout)


@unittest.skipUnless(NODE, "node not installed")
class Unread(unittest.TestCase):
    """A session that finished a turn while you looked elsewhere is "new"."""

    def test_first_sight_is_seen_and_a_later_finish_is_new(self):
        got = glance_js(
            "(() => { const a = {id: 'a', status: 'idle', status_changed_at: 100},"
            " b = {id: 'b', status: 'busy', status_changed_at: 90};"
            " seenAt = updateSeen({}, [a, b], null, 200); const first = [isUnread(a), isUnread(b)];"
            " b.status = 'idle'; b.status_changed_at = 150; seenAt = updateSeen(seenAt, [a, b], null, 210);"
            " const later = [isUnread(a), isUnread(b)];"
            " selected = 'b'; const whileOpen = isUnread(b);"
            " seenAt = updateSeen(seenAt, [a, b], 'b', 220); selected = 'a';"
            " return [first, later, whileOpen, isUnread(b)]; })()")
        self.assertEqual(got, [[False, False], [False, True], False, False])

    def test_gone_sessions_drop_out_but_an_empty_list_changes_nothing(self):
        got = glance_js(
            "[updateSeen({a: 5, b: 6}, [{id: 'a', status: 'busy'}], null, 9),"
            " updateSeen({a: 5}, [], null, 9)]")
        self.assertEqual(got, [{"a": 5}, {"a": 5}])

    def test_tiles_and_nodes_show_it(self):
        for needle in ('class="badge new"', "isUnread(s)", "kids.filter(isUnread)"):
            self.assertIn(needle, ui.SCRIPT)


@unittest.skipUnless(NODE, "node not installed")
class Attention(unittest.TestCase):
    def test_only_sessions_that_start_waiting_are_announced(self):
        got = glance_js(
            "(() => { const list = [{id: 'a', attention: true}, {id: 'b', attention: true}, {id: 'c'}];"
            " return [newlyWaiting(null, list).length,"
            " newlyWaiting(new Set(['a']), list).map(key)]; })()")
        self.assertEqual(got, [0, ["b"]])

    def test_a_session_that_just_started_is_switched_to(self):
        got = glance_js(
            "(() => { const now = 2e9, old = {id: 'old', startedAt: (now - 5000) * 1000},"
            " a = {id: 'a', startedAt: (now - 10) * 1000}, b = {id: 'b', startedAt: now - 5},"
            " stale = {id: 'stale', startedAt: (now - 1000) * 1000}, bare = {id: 'bare'};"
            " return [newlyStarted(null, [old, a], now),"
            " newlyStarted(new Set(['old']), [old, a, b], now),"
            " newlyStarted(new Set(['old']), [old, stale, bare], now),"
            " newlyStarted(new Set(['old', 'a']), [old, a], now)]; })()")
        self.assertEqual(got, [None, "b", None, None])
        self.assertIn("catchStarted(newlyStarted(", ui.SCRIPT)

    def test_title_counts_waiting_sessions(self):
        self.assertEqual(glance_js("[titleFor(0), titleFor(3)]"),
                         ["Imperatorium", "(3) Imperatorium"])

    def test_next_waiting_goes_longest_wait_first_and_wraps(self):
        got = glance_js(
            "(() => { const l = [{id: 'x'}, {id: 'new', attention: true, attention_since: 50},"
            " {id: 'old', attention: true, attention_since: 10}];"
            " return [nextWaiting(l, 'x'), nextWaiting(l, 'old'), nextWaiting(l, 'new'),"
            " nextWaiting([{id: 'x'}], 'x')]; })()")
        self.assertEqual(got, ["old", "new", "old", None])

    def test_the_window_wires_it_up(self):
        page = ui.render_shell()
        for needle in ("id=fav", "id=jumpbox", "id=jq", 'new EventSource("/api/stream")',
                       "openStream();", "gotSessions(", "goWaiting()"):
            self.assertIn(needle, page)


@unittest.skipUnless(NODE, "node not installed")
class HeavyWarnings(unittest.TestCase):
    def test_full_context_and_cold_large_cache_warn(self):
        got = glance_js(
            "[heavyWarnings({usage: {context_pct: 91}}, 100).map(h => h.kind),"
            " heavyWarnings({work: {context_tokens: 230000, cache_expires_at: 50}}, 100).map(h => h.short),"
            " heavyWarnings({work: {context_tokens: 230000, cache_expires_at: 500}}, 100).length,"
            " heavyWarnings({work: {context_tokens: 20000, cache_expires_at: 50, cold_tokens: 100000}}, 100).length,"
            " heavyWarnings({work: {context_tokens: 20000, cache_expires_at: 50}}, 100).map(h => h.short),"
            " heavyWarnings({work: {context_tokens: 20000, cache_expires_at: 50, cold_tokens: 0}}, 100).length,"
            " heavyWarnings({work: {cache_expires_at: 50}}, 100).length,"
            " heavyWarnings({usage: {context_pct: 40}}, 100).length]")
        self.assertEqual(got, [["context"], ["cold 230k"], 0, 0, ["cold 20k"], 1, 0, 0])

    def test_cold_cache_is_a_button_in_the_chat(self):
        got = glance_js(
            "[coldHtml({pid: 7, work: {context_tokens: 230000, cache_expires_at: 50, resume_cost_usd: 1.15}}, 100),"
            " coldHtml({work: {context_tokens: 230000, cache_expires_at: 50}}, 100),"
            " coldHtml({pid: 7, work: {context_tokens: 230000, cache_expires_at: 500}}, 100),"
            " coldHtml({remote: true, work: {context_tokens: 230000, cache_expires_at: 50}}, 100)]")
        self.assertIn("230k tokens", got[0])
        self.assertIn("$1.15", got[0])
        self.assertIn('data-coldact="new"', got[0])
        self.assertIn('data-coldact="new-close"', got[0])
        self.assertIn("a prompt entered as usual continues this session", got[0])
        self.assertNotIn("new-close", got[1])   # no process to close
        self.assertIn("cache-write price", got[1])
        self.assertEqual(got[2:], ["", ""])

    def test_the_cold_button_closes_until_the_next_expiry(self):
        got = glance_js(
            "(() => { const s = {id: 'a', pid: 7, work: {context_tokens: 230000, cache_expires_at: 50}};"
            " const open = coldHtml(s, 100, {});"
            " const closed = coldHtml(s, 100, {[coldKey(s)]: true});"
            " const later = coldHtml({...s, work: {...s.work, cache_expires_at: 90}}, 100, {[coldKey(s)]: true});"
            " return [open.includes('data-coldact=\"dismiss\"'), closed, later.length > 0]; })()")
        self.assertEqual(got, [True, "", True])

    def test_the_cold_button_sits_above_show_most_recent(self):
        js = ui.SCRIPT
        self.assertLess(js.index('<div class="cold" hidden>'), js.index('data-newest="1"'))


ITEMS = """[
 {speaker: 'you', blocks: [{kind: 'text', text: 'please fix the Parser'}]},
 {speaker: 'claude', blocks: [{kind: 'tool', name: 'Bash', summary: 'pytest -q'},
   {kind: 'result', text: 'boom', is_error: true}]},
 {speaker: 'claude', blocks: [{kind: 'thinking', text: 'parser again'},
   {kind: 'tool', name: 'Edit', summary: 'parser.py'}]},
 {speaker: 'claude', blocks: [{kind: 'result', text: 'ok', is_error: false}]}]"""


@unittest.skipUnless(NODE, "node not installed")
class FindInChat(unittest.TestCase):
    def test_text_tool_and_failure_searches(self):
        got = glance_js(
            "(() => { const it = %s; return [findHits(it, 'parser', ''), findHits(it, '', ''),"
            " findHits(it, '', 'tool'), findHits(it, 'edit', 'tool'), findHits(it, '', 'fail'),"
            " findHits(it, 'parser', '', (i, b) => b.kind !== 'thinking')]; })()" % ITEMS)
        self.assertEqual(got, [[0, 2], [], [1, 2], [2], [1], [0, 2]])

    def test_the_window_wires_it_up(self):
        for needle in ("id=cfind", "id=cfilter", "markFinds();", "resetFind();",
                       'data-i="${i}"'):
            self.assertIn(needle, ui.render_shell())


DIFF = "\\n".join([
    "diff --git a/src/app.py b/src/app.py", "index 1..2 100644", "--- a/src/app.py",
    "+++ b/src/app.py", "@@ -10,4 +10,5 @@ def main():", " keep", "-old", "+new", "+newer",
    " tail", "\\\\ No newline at end of file"])


@unittest.skipUnless(NODE, "node not installed")
class DiffComments(unittest.TestCase):
    def test_lines_know_their_file_and_number(self):
        got = glance_js("diffRows('%s').map(r => [r.cls, r.file || null, r.line || null, r.side || null])"
                        % DIFF)
        self.assertEqual(got, [
            ["f", None, None, None], ["", None, None, None], ["", None, None, None],
            ["", None, None, None], ["hh", None, None, None],
            ["", "src/app.py", 10, "new"], ["r", "src/app.py", 11, "old"],
            ["a", "src/app.py", 11, "new"], ["a", "src/app.py", 12, "new"],
            ["", "src/app.py", 13, "new"], ["", None, None, None]])

    def test_comments_become_one_prompt(self):
        got = glance_js(
            "reviewPrompt([{file: 'a.py', line: 3, side: 'new', code: '  x = `1`', text: 'why?'},"
            " {file: 'b.py', line: 9, side: 'old', code: '', text: 'keep this\\nplease'}])")
        self.assertIn("- `a.py:3` - `x = '1'`\n  why?", got)
        self.assertIn("- `b.py (removed line 9)`\n  keep this\n  please", got)
        self.assertTrue(got.startswith("Review comments on your uncommitted changes"))


@unittest.skipUnless(NODE, "node not installed")
class CostChart(unittest.TestCase):
    def test_one_bar_per_day_with_a_hover_label(self):
        got = glance_js(
            "(() => { const s = costChart([{date: '2026-10-01', cost: 0, tokens: 0},"
            " {date: '2026-10-02', cost: 12.5, tokens: 3100000}, {date: '2026-10-03', cost: 3, tokens: 9}]);"
            " return [(s.match(/class=\"bar\"/g) || []).length, (s.match(/class=\"hit\"/g) || []).length,"
            " s.includes('$12.50 · 3.1M tokens'), costChart([])]; })()")
        self.assertEqual(got, [2, 3, True, ""])

    def test_scale_rounds_up_to_a_tidy_number(self):
        self.assertEqual(glance_js("[niceMax(0.7), niceMax(13), niceMax(240), niceMax(5)]"),
                         [1, 20, 250, 5])


@unittest.skipUnless(NODE, "node is not installed")
class PlanInPlaceOfMeters(unittest.TestCase):
    """Off a Pro or Max plan there are no windows; the meter names the plan."""

    def run_js(self, u):
        js = ui.SHARED + "\nprocess.stdout.write(meterHtml(%s));" % json.dumps(u)
        out = subprocess.run([NODE, "-"], input=js, capture_output=True, text=True,
                             encoding="utf-8", timeout=30)
        if out.returncode:
            raise AssertionError(out.stderr)
        return out.stdout

    def test_bedrock_with_no_windows_shows_its_name_not_meters(self):
        got = self.run_js({"limits": {}, "plan": {"label": "AWS Bedrock",
                                                  "providers": ["bedrock"],
                                                  "alongside": False}})
        self.assertIn('class="plan"', got)
        self.assertIn("AWS Bedrock", got)
        self.assertNotIn('class="bar"', got)

    def test_nothing_known_stays_empty(self):
        self.assertEqual(self.run_js({"limits": {}, "plan": None}), "")

    def test_meters_keep_a_note_for_sessions_they_do_not_cover(self):
        got = self.run_js({"limits": {"five_hour": {"used": 40, "resets_at": None}},
                           "plan": {"label": "AWS Bedrock", "providers": ["bedrock"],
                                    "alongside": True}})
        self.assertIn('class="bar"', got)
        self.assertIn("also AWS Bedrock - no limits", got)

    def test_reset_time_is_in_the_tip_not_the_row(self):
        # Peer request 2026-10-18: the reset label made rows end at different
        # places; it moved to a hover tip, and the row ends with the bare
        # percentage in its own column.
        resets = int(time.time()) + 2 * 3600 + 5 * 60
        got = self.run_js({"limits": {"five_hour": {"used": 42.4, "resets_at": resets},
                                      "seven_day": {"used": 7, "resets_at": None}}})
        self.assertEqual(got.count('class="row"'), 2)
        self.assertIn('<span class="pct">42%</span>', got)
        self.assertIn('<span class="pct">7%</span>', got)
        tip = got[got.index('class="tip"'):]
        tip = tip[:tip.index('</div></div>')]
        self.assertIn("5-hour window", tip)
        self.assertIn("resets ", tip)
        self.assertRegex(tip, r"\(in 2h [45]m\)")
        self.assertIn("of the window elapsed", tip)
        row = got[:got.index('class="tip"')]
        self.assertNotIn("·", row)
        self.assertNotIn("title=", row)

    def test_a_window_waiting_after_its_reset_says_so(self):
        got = self.run_js({"limits": {"five_hour": {"used": 0, "resets_at": None, "reset_at": 1000,
                                                    "waiting": True, "pace": None}}})
        self.assertIn('class="row waiting"', got)
        self.assertIn('<span class="pct">0%</span>', got)
        self.assertIn("waiting for a new reading", got)
        self.assertNotIn("<b style", got)   # no time tick: the window's span is unknown

    def test_every_bar_gets_the_same_length(self):
        rule = ui.CSS[ui.CSS.index(".meters .rows{"):]
        rule = rule[:rule.index("}")]
        self.assertIn("grid-template-columns:auto 1fr minmax(4ch,auto)", rule)
        self.assertIn("grid-template-columns:subgrid", ui.CSS)


SPEND = {"global": {"windows": [
            {"name": "five_hour", "since": 1, "resets_at": 18001, "total_tokens": 17200000,
             "cost_usd": 8.86, "tokens": {"input": 1, "output": 2, "cache_read": 3, "cache_write": 4}},
            {"name": "seven_day", "since": 1, "resets_at": None, "total_tokens": 412000000,
             "cost_usd": 189.56, "tokens": {}}]},
         "current": {"sessions": 2, "total_tokens": 52600000, "cost_usd": 27.76, "tokens": {},
                     "each": [{"name": "c5", "total_tokens": 37500000, "cost_usd": 20.6}]},
         "complete": True}


MIXED = {"global": {"windows": [
            {"name": "five_hour", "since": 1, "resets_at": 18001, "total_tokens": 9000,
             "cost_usd": 1, "tokens": {}, "models": {
                 "claude-haiku-4-5": {"output": 1000, "total_tokens": 2000, "cost_usd": 0.1},
                 "claude-sonnet-4-5": {"output": 2000, "total_tokens": 3000, "cost_usd": 0.3},
                 "claude-sonnet-4-6": {"output": 1000, "total_tokens": 1000, "cost_usd": 0.1},
                 "claude-opus-4-1": {"output": 4000, "total_tokens": 4000, "cost_usd": 0.5}}}]},
         "current": {"sessions": 1, "total_tokens": 10, "cost_usd": 0, "tokens": {}, "each": [],
                     "models": {"mystery-1": {"output": 10, "total_tokens": 10, "cost_usd": 0}}},
         "complete": True}


@unittest.skipUnless(NODE, "node is not installed")
class SpendShown(unittest.TestCase):
    """Global and current-session spend in the meter and the matrix's status
    tile (peer request 2026-10-18)."""

    def meter(self, u, opts=None):
        js = ui.SHARED + "\nprocess.stdout.write(meterHtml(%s, %s));" % (json.dumps(u), json.dumps(opts))
        out = subprocess.run([NODE, "-"], input=js, capture_output=True, text=True,
                             encoding="utf-8", timeout=30)
        if out.returncode:
            raise AssertionError(out.stderr)
        return out.stdout

    def test_the_meter_shows_global_and_open_lines(self):
        got = self.meter({"limits": {"five_hour": {"used": 30, "resets_at": 18001}}, "spend": SPEND})
        self.assertIn('<span class="k">global</span>', got)
        self.assertIn("5h ≈$8.86 · 7d ≈$190", got)
        self.assertIn('<span class="k">open</span>', got)
        self.assertIn("2 sessions · ≈$27.76 · 53M tokens", got)
        self.assertIn("c5: 38M tokens · ≈$20.60", got)          # in the Open title
        self.assertIn("from this machine: 17M tokens · ≈$8.86", got)  # in the 5h tip
        self.assertIn("reset time not known yet", got)            # the 7d title

    def test_without_windows_the_month_still_shows(self):
        sp = dict(SPEND, **{"global": {"windows": [{"name": "month", "since": 1, "resets_at": None,
                                                     "total_tokens": 5, "cost_usd": 545.15, "tokens": {}}]}})
        got = self.meter({"limits": {}, "plan": {"label": "Claude Enterprise"}, "spend": sp})
        self.assertIn("Claude Enterprise", got)
        self.assertIn("month ≈$545", got)

    def test_the_status_tile_has_both_parts(self):
        html = matrix_js("process.stdout.write(JSON.stringify(statusTile(%s)));" % json.dumps(dict(
            {"cost_usd": 27.76, "costed": 2, "sessions": 2, "total_tokens": 52600000,
             "tokens": {}, "each": [], "limits": {}}, **{"global": SPEND["global"]})))
        self.assertLess(html.index("Current sessions"), html.index("<h2>Global</h2>"))
        for needle in ("each over its whole life", "5-hour window", "7-day window", "$189.56"):
            self.assertIn(needle, html)

    # Peer request 2026-10-18: the sidebar shows the bars alone and a click
    # opens the rest; the matrix tile leads with the meters and model mix.
    def test_everything_but_the_bars_is_extra(self):
        got = self.meter({"limits": {"five_hour": {"used": 30, "resets_at": 18001,
                                                   "pace": {"per_hour": 4}}},
                          "spend": MIXED})
        bars = got[:got.index('class="tip"')]
        self.assertIn('<span class="pct">30%</span>', bars)
        # Each piece after the bars is marked extra, which the closed meter hides.
        for piece in ('class="pace extra"', 'class="spend extra"', 'class="cap extra"',
                      'class="row mix extra"', 'class="legend extra"'):
            self.assertIn(piece, got)
        self.assertIn("#meter:not(.open) .extra{display:none}", ui.CSS)

    def test_the_mix_stacks_output_by_family_cheapest_first(self):
        got = self.meter({"limits": {}, "spend": MIXED})
        five = got[got.index('<span>5h</span>'):]
        five = five[:five.index('class="tip"')]
        # haiku 1000, sonnet 3000 (two models), opus 4000 of 8000 output.
        self.assertLess(five.index('class="i1" style="width:12.50%"'),
                        five.index('class="i2" style="width:37.50%"'))
        self.assertLess(five.index('class="i2"'), five.index('class="i3" style="width:50.00%"'))
        self.assertIn('<span class="pct">8.0k</span>', five)
        self.assertIn("sonnet: 3.0k out (38%)", got)
        # A model with no known price is grey, last.
        opened = got[got.index('<span>open</span>'):]
        self.assertIn('class="io"', opened[:opened.index('class="tip"')])
        legend = got[got.index('class="legend extra"'):]
        self.assertEqual([n for n in ("haiku", "sonnet", "opus", "fable", "other") if n in legend],
                         ["haiku", "sonnet", "opus", "other"])

    def test_the_matrix_leaves_out_the_spend_lines(self):
        got = self.meter({"limits": {"five_hour": {"used": 30}}, "spend": MIXED}, {"lines": False})
        self.assertNotIn('class="spend', got)
        self.assertIn('class="row mix extra"', got)

    def test_the_status_tile_leads_with_the_meters(self):
        html = matrix_js("process.stdout.write(JSON.stringify(statusTile(%s)));" % json.dumps(
            {"cost_usd": 1, "costed": 1, "sessions": 1, "total_tokens": 5, "tokens": {}, "each": [],
             "limits": {"five_hour": {"used": 42}}, "global": MIXED["global"],
             "models": MIXED["current"]["models"]}))
        self.assertLess(html.index('class="meters top"'), html.index("Current sessions"))
        self.assertIn('<span class="pct">42%</span>', html)
        self.assertIn("model mix", html)
        self.assertNotIn("Account limits", html)

    def test_a_click_folds_the_meter_and_it_is_remembered(self):
        shell = ui.render_shell()
        self.assertIn("id=meter class=meters tabindex=0 role=button aria-expanded=false", shell)
        self.assertIn('localStorage.getItem("imperatorium.meterOpen")', ui.SCRIPT)
        self.assertIn('$("#meter").addEventListener("click"', ui.SCRIPT)
        # The caret's escape survives the Python string (a lost backslash
        # once made it a control character and the literal "5BE").
        self.assertIn('#meter::after{content:"\\25BE"', ui.CSS)
        self.assertFalse([c for c in ui.CSS if ord(c) < 32 and c not in "\n\t"])


def presses_js(tail):
    """The press-hold helpers, say/showSaid, with a stub DOM; `tail` after them."""
    a = ui.SCRIPT.index("const pressedIn")
    body = ui.SCRIPT[a:ui.SCRIPT.index("\nfunction drawSessions")]
    js = ("const line = {textContent: ''}; const $ = () => line;"
          " let selected = 'a'; const timers = []; const setTimeout = f => timers.push(f);\n" + body
          + "\nprocess.stdout.write(JSON.stringify(" + tail + "));")
    out = subprocess.run([NODE, "-"], input=js, capture_output=True, text=True,
                         encoding="utf-8", timeout=30)
    if out.returncode:
        raise AssertionError(out.stderr)
    return json.loads(out.stdout)


@unittest.skipUnless(NODE, "node not installed")
class SendLine(unittest.TestCase):
    """The line under the prompt box belongs to the session it is about."""

    def test_switching_hides_it_and_switching_back_shows_it(self):
        got = presses_js(
            "(() => { say('refused: no inbox'); const a = line.textContent;"
            " selected = 'b'; showSaid(); const b = line.textContent;"
            " selected = 'a'; showSaid(); return [a, b, line.textContent]; })()")
        self.assertEqual(got, ["refused: no inbox", "", "refused: no inbox"])

    def test_a_reply_after_a_switch_is_kept_for_its_session(self):
        got = presses_js(
            "(() => { say('sending…', 'a'); selected = 'b'; showSaid();"
            " say('refused', 'a'); const onB = line.textContent;"
            " selected = 'a'; showSaid(); return [onB, line.textContent]; })()")
        self.assertEqual(got, ["", "refused"])


@unittest.skipUnless(NODE, "node not installed")
class HeldRedraws(unittest.TestCase):
    """A redraw between press and release would eat the click."""

    def test_a_redraw_waits_for_the_release_and_then_runs_once(self):
        got = presses_js(
            "(() => { let n = 0; const draw = () => n++;"
            " pressedIn.add('#sessions');"
            " const a = held('#sessions', draw), b = held('#sessions', draw), other = held('#status', draw);"
            " releasePresses(); const queued = timers.length; timers.forEach(f => f());"
            " return [a, b, other, queued, n, held('#sessions', draw)]; })()")
        # held twice, once queued; another panel is not held; after the
        # release the draw runs once, and nothing is held any more.
        self.assertEqual(got, [True, True, False, 1, 1, False])

    def test_set_text_leaves_equal_text_alone(self):
        got = presses_js(
            "(() => { let writes = 0; const el = {_t: 'x', get textContent(){ return this._t; },"
            " set textContent(v){ writes++; this._t = v; }};"
            " setText(el, 'x'); setText(el, 'y'); return [writes, el.textContent]; })()")
        self.assertEqual(got, [1, "y"])


class RedrawGuards(unittest.TestCase):
    """Panels redrawn on a timer replace their content only when it changed."""

    def test_timer_panels_are_held_while_pressed(self):
        js = ui.SCRIPT
        for fn, sel in (("drawSessions", "#sessions"), ("drawStatus", "#status"), ("drawProcs", "#procs")):
            start = js.index("function %s(){" % fn)
            self.assertIn('held("%s", %s)' % (sel, fn), js[start:start + 200], fn)
        start = js.index("function drawLog(force){")
        self.assertIn('held("#log", () => drawLog(force))', js[start:start + 200])
        self.assertIn('["#sessions", "#status", "#procs", "#log"].forEach(holdPresses)', js)
        self.assertIn('window.addEventListener("pointerup", releasePresses, true)', js)

    def test_timer_panels_compare_against_what_they_drew(self):
        js = ui.SCRIPT
        for el in ("#sessions", "#proctabs", "#meter", "#activity", "#curbody"):
            self.assertIn('if ($("%s").dataset.sig !== ' % el, js, el)
        self.assertIn("if (box.dataset.sig !== html){ box.innerHTML = html; box.dataset.sig = html; }", js)
        # innerHTML comes back in the browser's own quoting, so it is never
        # what was drawn: comparing against it redrew every time.
        self.assertNotIn('.innerHTML !== ', js)


if __name__ == "__main__":
    unittest.main()


@unittest.skipUnless(NODE, "node is not installed")
class AwaitedPanel(unittest.TestCase):
    """The tabs under the chat for what the session is waiting on."""

    def view(self, tasks, pick="auto"):
        a = ui.SCRIPT.index("function procView(")
        js = (ui.SCRIPT[a:ui.SCRIPT.index("function procWanted(")] +
              "\nprocess.stdout.write(JSON.stringify(procView(%s, %s)));"
              % (json.dumps(tasks), json.dumps(pick)))
        out = subprocess.run([NODE, "-"], input=js, capture_output=True, text=True,
                             encoding="utf-8", timeout=30)
        if out.returncode:
            raise AssertionError(out.stderr)
        return json.loads(out.stdout)

    A = {"id": "ba", "kind": "shell", "label": "npm test", "started": 1, "mtime": 50, "text": "a"}
    B = {"id": "bb", "kind": "monitor", "label": "watch log", "started": 2, "mtime": 40, "text": "b"}

    def test_nothing_awaited_shows_nothing(self):
        self.assertIsNone(self.view([])["show"])

    def test_one_task_has_no_auto_tab(self):
        got = self.view([self.A])
        self.assertEqual([t["id"] for t in got["tabs"]], ["ba"])
        self.assertEqual(got["show"]["id"], "ba")
        self.assertTrue(got["tabs"][0]["on"])

    def test_several_put_auto_first_and_follow_the_latest_output(self):
        got = self.view([self.A, self.B])
        self.assertEqual([t["id"] for t in got["tabs"]], ["auto", "ba", "bb"])
        self.assertEqual(got["show"]["id"], "ba")
        self.assertTrue(got["tabs"][1]["live"])
        later = dict(self.B, mtime=60)
        self.assertEqual(self.view([self.A, later])["show"]["id"], "bb")

    def test_a_chosen_tab_sticks_until_its_task_ends(self):
        self.assertEqual(self.view([self.A, self.B], "bb")["show"]["id"], "bb")
        got = self.view([self.A, self.B], "gone")
        self.assertEqual((got["pick"], got["show"]["id"]), ("auto", "ba"))

    def test_panel_sits_directly_under_the_chat(self):
        page = ui.render_shell()
        self.assertIn("<div id=log></div><div id=procs hidden>", page)
