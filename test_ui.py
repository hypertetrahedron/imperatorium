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


class Controls(unittest.TestCase):
    def test_shell_has_the_new_controls(self):
        html = ui.render_shell()
        for needle in ("id=curbtn", "Show Current Prompt", "id=jprev", "id=jnext",
                       "id=current"):
            self.assertIn(needle, html)


if __name__ == "__main__":
    unittest.main()
