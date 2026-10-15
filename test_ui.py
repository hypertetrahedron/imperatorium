"""The window's text rendering, run under node against the real SCRIPT.

The renderer is JavaScript inside ui.py, so it is exercised by slicing the
Markdown and table functions out of the shipped string rather than by a copy that could
drift from it. Skipped where node is not installed.
"""
import json
import shutil
import subprocess
import unittest

from ccontrol import ui

NODE = shutil.which("node")


def render(text):
    start = ui.SCRIPT.index("const esc =")
    esc = ui.SCRIPT[start:ui.SCRIPT.index("\n\n", start)]
    body = ui.SCRIPT[ui.SCRIPT.index("const TABLE_SEP"):ui.SCRIPT.index("function renderBlocks")]
    js = esc + "\n" + body + "\nprocess.stdout.write(renderText(%s));" % json.dumps(text)
    out = subprocess.run([NODE, "-e", js], capture_output=True, text=True,
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
        out = subprocess.run([NODE, "-e", js], capture_output=True, text=True,
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

    def run_js(self, group, sessions, folded=()):
        s = ui.SCRIPT
        part = lambda a, b: s[s.index(a):s.index(b, s.index(a))]
        js = ("const store = {'ccontrol.folded': %s};\n"
              "const localStorage = {getItem: k => store[k] || null, setItem: (k, v) => { store[k] = v; }};\n"
              % json.dumps(json.dumps(list(folded)))
              + part("const esc =", "\n\n") + "\n" + part("const folder =", "\n") + "\n"
              + part("const nameOf =", "\nfunction groupOf") + "\n"
              + part("function tree(", "\nfunction drawSessions") + "\n"
              "const stateOf = s => [s.attention ? 'd-attn' : 'd-idle', 'x'];\n"
              "const tile = (s, short) => '[' + (short && s.short_name || nameOf(s)) + ']';\n"
              "const prefs = {group: %s};\n"
              "process.stdout.write(tree(%s, 'idle'));" % (json.dumps(group), json.dumps(sessions)))
        out = subprocess.run([NODE, "-e", js], capture_output=True, text=True,
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

    def test_folded_node_keeps_only_who_needs_you(self):
        html = self.run_js("state", self.SESSIONS, folded=["hai-proof"])
        self.assertIn('class="fam shut"', html)
        self.assertIn('<div class="kids">[fizzy-wondering-lark]</div>', html)
        self.assertNotIn("[5b]", html)
        self.assertEqual(html.count('class="dot '), 2)

    def test_folded_node_with_nobody_waiting_has_no_tiles(self):
        calm = [dict(x, attention=False) for x in self.SESSIONS]
        html = self.run_js("state", calm, folded=["hai-proof"])
        self.assertNotIn("kids", html)

    def test_a_lone_session_is_a_plain_tile(self):
        self.assertEqual(self.run_js("state", self.SESSIONS[1:2]), "[strata-0b]")

    def test_grouped_by_folder_every_project_is_a_node(self):
        html = self.run_js("folder", self.SESSIONS)
        self.assertIn('<div class="kids">[5b][fizzy-wondering-lark]</div>', html)
        self.assertIn('data-fam="strata"', html)
        self.assertTrue(html.endswith('<div class="kids">[0b]</div>'))

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
        out = subprocess.run([NODE, "-e", js], capture_output=True, text=True,
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
    """Cache minutes left and subagents/shells/monitors/processes, as on a tile."""

    def parts(self, work, now=1000):
        start = ui.SCRIPT.index("const esc =")
        esc = ui.SCRIPT[start:ui.SCRIPT.index("\n\n", start)]
        w = ui.SCRIPT.index("function workParts(")
        js = (esc + "\n" + ui.SCRIPT[w:ui.SCRIPT.index("function tile(")] +
              "\nprocess.stdout.write(JSON.stringify(workParts(%s, %d).map(p => p.text)));"
              % (json.dumps(work), now))
        out = subprocess.run([NODE, "-e", js], capture_output=True, text=True,
                             encoding="utf-8", timeout=30)
        if out.returncode:
            raise AssertionError(out.stderr)
        return json.loads(out.stdout)

    def test_the_example(self):
        self.assertEqual(self.parts({"cache_expires_at": 1000 + 58 * 60 - 10, "alive": True,
                                     "agents": 0, "shells": 2, "monitors": 1,
                                     "processes": 0}), ["-58", "0/2/1/0"])

    def test_minutes_round_up_and_an_expired_cache_shows_nothing(self):
        self.assertEqual(self.parts({"cache_expires_at": 1030, "alive": True}),
                         ["-1", "0/0/0/0"])
        self.assertEqual(self.parts({"cache_expires_at": 900, "alive": True}), ["0/0/0/0"])

    def test_an_ended_session_has_no_counts(self):
        self.assertEqual(self.parts({"cache_expires_at": 2000, "alive": False}), ["-17"])
        self.assertEqual(self.parts(None), [])

    def test_tile_and_status_line_both_use_it(self):
        self.assertIn('workHtml(s.work, Date.now() / 1000, "badge work")', ui.SCRIPT)
        self.assertIn('workHtml(s.work, Date.now() / 1000, "work", "cache")', ui.SCRIPT)


class StatusPanel(unittest.TestCase):
    """The panel under the send buttons, rendered from a row and its status."""

    def run_js(self, row, status, folded=False):
        start = ui.SCRIPT.index("const esc =")
        esc = ui.SCRIPT[start:ui.SCRIPT.index("\n\n", start)]
        a = ui.SCRIPT.index("function kfmt(")
        body = ui.SCRIPT[a:ui.SCRIPT.index("function drawStatus")]
        w = ui.SCRIPT.index("function workParts(")
        work = ui.SCRIPT[w:ui.SCRIPT.index("function tile(")]
        js = (esc + "\nconst age = t => 'AGE';\nDate.now = () => 1000 * 1000;\n"
              "const stateOf = s => ['d-busy', s.label];\n" + work + body +
              "\nprocess.stdout.write(statusHtml(%s, %s, 1000, %s));"
              % (json.dumps(row), json.dumps(status), json.dumps(folded)))
        out = subprocess.run([NODE, "-e", js], capture_output=True, text=True,
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

    def test_the_summary_line_ends_with_the_tiles_totals_and_no_second_countdown(self):
        row = dict(self.ROW, work={"cache_expires_at": 1000 + 300, "alive": True,
                                   "agents": 1, "shells": 2, "monitors": 1, "processes": 0})
        html = self.run_js(row, self.STATUS)
        head = html[:html.index('<div class="bd">')]
        self.assertRegex(head, r'cache -5m · \$3\.46 · '
                               r'<span class="work busy"[^>]*>1/2/1/0</span></span>')
        self.assertNotIn("work cache", head)

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
        out = subprocess.run([NODE, "-e", js], capture_output=True, text=True, encoding="utf-8",
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
    def test_shell_has_the_new_controls(self):
        html = ui.render_shell()
        for needle in ("id=curbtn", "Show Current Prompt", "id=jprev", "id=jnext",
                       "id=current", "id=strip", "id=shape"):
            self.assertIn(needle, html)

    def test_jump_sits_in_the_strip_between_chat_and_prompt(self):
        html = ui.render_shell()
        strip = html.index("id=strip")
        self.assertLess(html.index("id=log"), strip)
        self.assertLess(strip, html.index("id=shape"))
        self.assertLess(html.index("id=shape"), html.index("id=jump"))
        self.assertLess(html.index("id=jump"), html.index("id=ask"))


if __name__ == "__main__":
    unittest.main()
