// The window's text rendering (shared.js / app.js), run against the real script.
// Ported from test_ui.py: TableTests, MarkdownTests, Highlighting.
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { runRenderer as run, render, cat, j, assertIn, assertNotIn } from "./load.mjs";

const UNESCAPE = { lt: "<", gt: ">", amp: "&", quot: '"', apos: "'" };
const unescapeHtml = (s) =>
  s.replace(/&(?:#x([0-9a-f]+)|#(\d+)|(\w+));/gi, (m, hex, dec, name) =>
    hex ? String.fromCodePoint(parseInt(hex, 16))
      : dec ? String.fromCodePoint(+dec)
      : name in UNESCAPE ? UNESCAPE[name] : m);

describe("TableTests", () => {
  it("plain text unchanged", () => {
    assert.equal(render("hello\nworld"), '<div class="txt">hello\nworld</div>');
  });

  it("table between prose", () => {
    const html = render("Here:\n\n| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\nDone.");
    assertIn('<div class="txt">Here:</div><div class="tbl"><table>', html);
    assertIn("<thead><tr><th>A</th><th>B</th></tr></thead>", html);
    assertIn("<tr><td>1</td><td>2</td></tr><tr><td>3</td><td>4</td></tr>", html);
    assert.ok(html.endsWith('</table></div><div class="txt">Done.</div>'));
  });

  it("alignment", () => {
    const html = render("| l | c | r |\n|:--|:-:|--:|\n| a | b | c |");
    assertIn('<td style="text-align:left">a</td>', html);
    assertIn('<td style="text-align:center">b</td>', html);
    assertIn('<td style="text-align:right">c</td>', html);
  });

  it("without outer pipes", () => {
    const html = render("A | B\n--- | ---\n1 | 2");
    assertIn("<th>A</th><th>B</th>", html);
    assertIn("<td>1</td><td>2</td>", html);
  });

  it("escaped pipe and inline", () => {
    const html = render("| k | v |\n|---|---|\n| `a\\|b` | **yes** |");
    assertIn("<td><code>a|b</code></td><td><b>yes</b></td>", html);
  });

  it("cells are escaped", () => {
    const html = render("| x |\n|---|\n| <script> |");
    assertIn("<td>&lt;script&gt;</td>", html);
    assertNotIn("<script>", html);
  });

  it("short and long rows fit header", () => {
    const html = render("| a | b |\n|---|---|\n| 1 |\n| 1 | 2 | 3 |");
    assertIn("<tr><td>1</td><td></td></tr>", html);
    assertIn("<tr><td>1</td><td>2</td></tr>", html);
  });

  it("inside fence left alone", () => {
    const text = "```\n| a | b |\n|---|---|\n| 1 | 2 |\n```";
    assertNotIn("<table>", render(text));
  });

  it("mismatched delimiter is not a table", () => {
    assertNotIn("<table>", render("| a | b |\n|---|\n| 1 | 2 |"));
  });

  it("pipe in prose is not a table", () => {
    assertNotIn("<table>", render("run a | b\nthen c"));
  });
});

describe("MarkdownTests", () => {
  it("bold italic strike in prose", () => {
    const html = render("this is **bold**, *italic*, _also_ and ~~gone~~");
    assert.equal(html, cat('<div class="txt">this is <b>bold</b>, <i>italic</i>, ',
                           "<i>also</i> and <s>gone</s></div>"));
  });

  it("snake case and arithmetic are not emphasis", () => {
    const html = render("call my_func_name with 2 * 3 * 4 in some_file.py");
    assertNotIn("<i>", html);
    assertIn("my_func_name", html);
  });

  it("nothing inside inline code is formatted", () => {
    assertIn("<code>a **b** c</code>", render("run `a **b** c` now"));
  });

  it("headings", () => {
    const html = render("## Summary\ntext");
    assertIn('<div class="h h2">Summary</div>', html);
    assertIn('<div class="txt">text</div>', html);
  });

  it("bullet list with inline formatting", () => {
    const html = render("Changes:\n- **one**\n- two\n\nDone.");
    assertIn('<div class="md"><ul><li><b>one</b></li><li>two</li></ul></div>', html);
    assert.ok(html.endsWith('<div class="txt">Done.</div>'));
  });

  it("numbered list keeps its start", () => {
    assertIn('<ol start="3"><li>c</li><li>d</li></ol>', render("3. c\n4. d"));
  });

  it("nested list", () => {
    assertIn("<ul><li>a<ul><li>b</li></ul></li><li>c</li></ul>", render("- a\n  - b\n- c"));
  });

  it("a blank line between items keeps one list", () => {
    assert.equal(render("- a\n\n- b").split("<ul>").length - 1, 1);
  });

  it("fenced code is escaped and left raw", () => {
    assert.equal(render("```python\nx = **y** <z>\n```"),
                 '<pre class="code" data-lang="py">x = **y** &lt;z&gt;</pre>');
  });

  it("unclosed fence runs to the end", () => {
    assertIn('<pre class="code">a\n- b</pre>', render("```\na\n- b"));
  });

  it("blockquote and rule", () => {
    const html = render("> quoted **x**\n\n---\nafter");
    assertIn('<blockquote><div class="txt">quoted <b>x</b></div></blockquote>', html);
    assertIn("<hr>", html);
  });

  it("links only for http", () => {
    const html = render("[docs](https://example.com/a_b) and [x](javascript:alert(1))");
    assertIn(cat('<a href="https://example.com/a_b" target="_blank" ', 'rel="noopener">docs</a>'), html);
    assertNotIn('href="javascript', html);
  });

  it("bare url is linked without trailing punctuation", () => {
    assertIn(">https://github.com/a/b_c_d</a>.", render("see https://github.com/a/b_c_d."));
  });

  it("html in prose is escaped", () => {
    const html = render("**<img src=x onerror=alert(1)>**");
    assertIn("<b>&lt;img src=x onerror=alert(1)&gt;</b>", html);
    assertNotIn("<img", html);
  });
});

describe("Highlighting", () => {
  /** Marks are spans around the text: they never change what it says. */
  const strip = (html) => unescapeHtml(html.replace(/<[^>]+>/g, ""));

  it("python tokens", () => {
    const html = render('```python\ndef f(x):\n    return "hi"  # done\n```');
    assertIn('<span class="tk-kw">def</span> <span class="tk-fn">f</span>(x):', html);
    assertIn('<span class="ig">    </span><span class="tk-kw">return</span>', html);
    assertIn('<span class="tk-str">&quot;hi&quot;</span>', html);
    assertIn('<span class="tk-com"># done</span>', html);
  });

  it("text is unchanged by marks", () => {
    for (const src of ['```js\nconst a = `x ${y}` // c\nif (a) { b(1.5e3); }\n```',
                       "```\n$ git commit -m \"msg\" --amend\n```",
                       "```powershell\nGet-ChildItem -Path $env:TEMP # list\n```",
                       '```json\n{"a": [1, true, null]}\n```',
                       "```yaml\nkey: value # c\n- item: 2\n```",
                       "```sql\nSELECT a FROM t WHERE b = 'x';\n```",
                       "```\n<div class=\"a\">t</div>\n```",
                       "```\n\tdeep\n\t\tdeeper\n```"]) {
      const rest = src.slice(src.indexOf("\n") + 1);
      const body = rest.slice(0, rest.lastIndexOf("\n"));
      assert.equal(strip(render(src)), body, src);
    }
  });

  it("guesses the language", () => {
    assert.deepEqual(run(cat('["def f():\\n  pass", "const x = 1;", "$ ls -la", \'{"a": 1}\', ',
                             '"Get-Item x", "plain words here"].map(guessLang)')),
                     ["py", "c", "sh", "json", "ps", ""]);
  });

  it("a named unknown language is left plain", () => {
    assert.equal(render("```text\nif x then y\n```"), '<pre class="code">if x then y</pre>');
  });

  it("diff block", () => {
    const html = render("```diff\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n same\n```");
    assertIn('<pre class="code dif"><span class="dl-f">--- a/x</span>', html);
    assertIn('<span class="dl-r">-old</span>\n<span class="dl-a">+new</span>\n same', html);
  });

  it("prose marks", () => {
    const html = render(cat("See imperatorium/ui.py:120 and C:\\Users\\x\\a.txt, commit a265fcf, #42, ",
                            "run `git status`, `docs/ui-names.md`, 1.5k tokens for $0.42 in 3s."));
    assertIn(cat('<span class="pth" data-copy="imperatorium/ui.py:120" title="click to open, show in its folder or copy">',
                 "imperatorium/ui.py:120</span>"), html);
    assertIn('data-copy="C:\\Users\\x\\a.txt"', html);
    assertIn('<span class="ref">a265fcf</span>', html);
    assertIn('<span class="ref">#42</span>', html);
    assertIn('<code class="cmd">git status</code>', html);
    assertIn('<code class="pth" data-copy="docs/ui-names.md"', html);
    assertIn('<span class="qty">1.5k tokens</span>', html);
    assertIn('<span class="qty">$0.42</span>', html);
    assertIn('<span class="qty">3s</span>', html);
  });

  it("words that are not paths", () => {
    const html = render("and/or, read/write, 24/7, e.g. this, 2026/11/03, version 1.2");
    assertNotIn("pth", html);
    assertNotIn("ref", html);
  });

  it("marks leave emphasis and snake case alone", () => {
    const html = render("**bold** a_b_c and _it_ in my_file.py");
    assertIn("<b>bold</b>", html);
    assertIn("a_b_c", html);
    assertIn("<i>it</i>", html);
    assertIn('data-copy="my_file.py"', html);
  });

  it("signals", () => {
    const html = render("Ran 1027 tests: OK, 2 failed, TypeError raised, a DeprecationWarning");
    assertIn('<span class="sg-pass">Ran 1027 tests</span>', html);
    assertIn('<span class="sg-pass">OK</span>', html);
    assertIn('<span class="sg-err">2 failed</span>', html);
    assertIn('<span class="sg-err">TypeError</span>', html);
    assertIn('<span class="sg-warn">DeprecationWarning</span>', html);
  });

  it("question lines", () => {
    const html = render("I did it.\nShall I commit?\n- or this one?");
    assertIn('<span class="ask">Shall I commit?</span>', html);
    assertIn('<li><span class="ask">or this one?</span>', html);
    assertNotIn('<span class="ask">I did it.', html);
  });

  it("html in text is still escaped", () => {
    const html = render("<img src=x onerror=alert(1)> at a/b.py `<b>`");
    assertNotIn("<img", html);
    assertIn("<code>&lt;b&gt;</code>", html);
  });

  it("tool lines", () => {
    const got = run(cat('[toolHtml({name: "Read", summary: "C:\\\\My Docs\\\\a.py"}), ',
                        'toolHtml({name: "Grep", summary: "def \\\\w+"}), ',
                        'toolHtml({name: "Edit", summary: "a.py", diff: "-x\\n+y"}), ',
                        'toolHtml({name: "Bash", summary: "cd x && git log --oneline \\"a b\\""})]'));
    assertIn('<span class="pth tp" data-copy="C:\\My Docs\\a.py"', got[0]);
    assertIn('<span class="tq">def \\w+</span>', got[1]);
    assertIn('<pre class="code dif tdiff"><span class="dl-r">-x</span>\n<span class="dl-a">+y</span></pre>', got[2]);
    assert.equal(strip(got[3]), 'Bash cd x && git log --oneline "a b"');
    assertIn('<span class="tc">git</span>', got[3]);
    assertIn('<span class="tf">--oneline</span>', got[3]);
    assertIn('<span class="tq">&quot;a b&quot;</span>', got[3]);
  });

  it("results", () => {
    const got = run(cat('(toolNames.set("t1", "Read"), [resultHtml({id: "t1", text: "x.py:3: Error", truncated: true}), ',
                        'resultHtml({id: "t2", text: "fine", is_error: true})])'));
    assertIn('class="res quiet cut"', got[0]);
    assertIn('<span class="sg-err">Error</span>', got[0]);
    assertIn("cut short", got[0]);
    assertIn('class="res err"', got[1]);
  });
});

// 2026-11-25: esc() is safe inside a quoted attribute; a tile's doing line
// (a Bash command) once could close title="..." and add an event handler.
describe("AttributeEscaping", () => {
  it("quotes cannot leave an attribute", () => {
    const out = run(j(`x" onmouseover="alert(1)' y`).replace(/^/, "esc(") + ")");
    assert.ok(!out.includes('"') && !out.includes("'"), out);
    assert.equal(out, "x&quot; onmouseover=&quot;alert(1)&#39; y");
  });
});
