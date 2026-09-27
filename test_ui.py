"""The window's text rendering, run under node against the real SCRIPT.

The renderer is JavaScript inside ui.py, so it is exercised by slicing the
table functions out of the shipped string rather than by a copy that could
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


if __name__ == "__main__":
    unittest.main()
