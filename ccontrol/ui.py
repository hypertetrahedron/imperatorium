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
import os


CSS = """
/* Atkinson Hyperlegible Next and Mono (SIL OFL 1.1, licences in fonts/),
   served from this package so the window needs no network. */
@font-face{font-family:'Atkinson Hyperlegible Mono';font-style:normal;font-weight:200 800;font-display:swap;
  src:url(/fonts/mono-latin-ext.woff2) format('woff2');
  unicode-range:U+0100-02BA, U+02BD-02C5, U+02C7-02CC, U+02CE-02D7, U+02DD-02FF, U+0304, U+0308, U+0329, U+1D00-1DBF, U+1E00-1E9F, U+1EF2-1EFF, U+2020, U+20A0-20AB, U+20AD-20C4, U+2113, U+2C60-2C7F, U+A720-A7FF}
@font-face{font-family:'Atkinson Hyperlegible Mono';font-style:normal;font-weight:200 800;font-display:swap;
  src:url(/fonts/mono-latin.woff2) format('woff2');
  unicode-range:U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, U+02DC, U+0304, U+0308, U+0329, U+2000-206F, U+20AC, U+2122, U+2191, U+2193, U+2212, U+2215, U+FEFF, U+FFFD}
@font-face{font-family:'Atkinson Hyperlegible Next';font-style:italic;font-weight:200 800;font-display:swap;
  src:url(/fonts/next-italic-latin-ext.woff2) format('woff2');
  unicode-range:U+0100-02BA, U+02BD-02C5, U+02C7-02CC, U+02CE-02D7, U+02DD-02FF, U+0304, U+0308, U+0329, U+1D00-1DBF, U+1E00-1E9F, U+1EF2-1EFF, U+2020, U+20A0-20AB, U+20AD-20C4, U+2113, U+2C60-2C7F, U+A720-A7FF}
@font-face{font-family:'Atkinson Hyperlegible Next';font-style:italic;font-weight:200 800;font-display:swap;
  src:url(/fonts/next-italic-latin.woff2) format('woff2');
  unicode-range:U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, U+02DC, U+0304, U+0308, U+0329, U+2000-206F, U+20AC, U+2122, U+2191, U+2193, U+2212, U+2215, U+FEFF, U+FFFD}
@font-face{font-family:'Atkinson Hyperlegible Next';font-style:normal;font-weight:200 800;font-display:swap;
  src:url(/fonts/next-latin-ext.woff2) format('woff2');
  unicode-range:U+0100-02BA, U+02BD-02C5, U+02C7-02CC, U+02CE-02D7, U+02DD-02FF, U+0304, U+0308, U+0329, U+1D00-1DBF, U+1E00-1E9F, U+1EF2-1EFF, U+2020, U+20A0-20AB, U+20AD-20C4, U+2113, U+2C60-2C7F, U+A720-A7FF}
@font-face{font-family:'Atkinson Hyperlegible Next';font-style:normal;font-weight:200 800;font-display:swap;
  src:url(/fonts/next-latin.woff2) format('woff2');
  unicode-range:U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, U+02DC, U+0304, U+0308, U+0329, U+2000-206F, U+20AC, U+2122, U+2191, U+2193, U+2212, U+2215, U+FEFF, U+FFFD}

/* Slate neutrals with one shared tint; state colours of equal weight from a
   colour-blind-safe set (amber, blue, teal, vermillion), each also told apart
   by the shape of its dot. Every state colour is 4.6:1 or better on the
   sidebar and the selection in both themes. */
:root{
  color-scheme: light dark;
  --ui:"Atkinson Hyperlegible Next",ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif;
  --mono:"Atkinson Hyperlegible Mono",Consolas,ui-monospace,monospace;
  --bg:#f5f7f9; --panel:#fcfdff; --side:#edf0f5; --ink:#192029; --muted:#565e69;
  --line:#ccd1d9; --warn:#9f4500; --warnbg:#fbeee3; --ok:#00736f; --busy:#2863ab;
  --sel:#dde9f9; --code:#e8ebef; --fail:#af2843; --done:#8a929d; --onaccent:#fff;
  /* The model mix, after claude-lens: an ironbow ramp in cost order, darkest
     cheapest. --iron-other is a model with no known price, off the ramp. */
  --iron-1:#33206b; --iron-2:#93206e; --iron-3:#cc3a17; --iron-4:#b87b00; --iron-other:#8a8880;
}
@media (prefers-color-scheme: dark){
  :root{ --bg:#0f1318; --panel:#1a1e25; --side:#14191e; --ink:#e4e8ed;
         --muted:#a5abb4; --line:#30363e; --warn:#f7ab5d; --warnbg:#2e2112;
         --ok:#6fcabf; --busy:#79b6f4; --sel:#202f42; --code:#0a0e12;
         --fail:#f2808b; --done:#6c737d; --onaccent:#0f1318;
         --iron-1:#7551e0; --iron-2:#a52a86; --iron-3:#ef6a1a; --iron-4:#ffcf4d; --iron-other:#6f6d66}
}
*{box-sizing:border-box}
html,body{height:100%}
body{margin:0;background:var(--bg);color:var(--ink);overflow:hidden;
  font:14px/1.55 var(--ui);font-variant-numeric:tabular-nums}
/* Form controls take the browser's own font unless told otherwise. */
button,input,select,textarea{font-family:inherit;font-variant-numeric:inherit}
[hidden]{display:none!important}
#app{display:flex;height:100%}

#side{width:270px;flex:none;background:var(--side);border-right:1px solid var(--line);
  display:flex;flex-direction:column}
#side h1{font-size:12px;text-transform:uppercase;letter-spacing:.07em;
  color:var(--muted);margin:0;padding:14px 14px 8px;
  display:flex;justify-content:space-between;align-items:center}
button.tiny{background:transparent;border:1px solid var(--line);color:var(--muted);
  border-radius:4px;padding:1px 6px;font-size:12px;font-family:inherit;text-transform:none;
  letter-spacing:0}
#picker{padding:0 10px 8px;border-bottom:1px solid var(--line)}
#find{width:100%;background:var(--panel);color:var(--ink);border:1px solid var(--line);
  border-radius:5px;padding:5px 7px;font-size:13px;font-family:inherit}
#find:focus{outline:none;border-color:var(--busy)}
#projects{max-height:210px;overflow-y:auto;margin-top:4px}
.p{padding:5px 7px;border-radius:4px;cursor:pointer;font-size:13px}
.p:hover,.p.on{background:var(--sel)}
.p small{color:var(--muted);display:block;font-size:12px}
#sessions{overflow-y:auto;flex:1}
.s{padding:9px 13px;border-bottom:1px solid var(--line);cursor:pointer}
.s:hover{background:var(--panel)}
.s.on{background:var(--sel)}
.s.attn{border-left:3px solid var(--warn);padding-left:10px}
/* First line: dot, name, why (when the dot does not say it), then the
   signals in fixed order on the right. The name keeps its room; the reason
   gives way first. Every tile has a second line, so closed tiles are all
   one height. */
.s .t1{display:flex;align-items:center;gap:6px;min-width:0}
.s .t1 .dot{margin-right:0}
.s .n{flex:0 1 auto;min-width:3em;font-weight:600;font-size:13.5px;
  white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.s .why{flex:0 100 auto;min-width:0;color:var(--muted);font-size:12px;
  white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.s.attn .why{color:var(--warn)}
.s.fail .why{color:var(--fail)}
.s .sig{flex:none;margin-left:auto;display:flex;align-items:center;font-variant-numeric:tabular-nums}
.s .w,.s .d{height:17px;line-height:17px;margin-top:2px}
.kids .s.attn,.kids .s.fail{padding-left:8px}
.s .m{display:flex;align-items:center;gap:6px}
.s .m .det{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;font-variant-numeric:tabular-nums}
.s .m .acts{display:flex;flex:none;gap:3px}
/* Only the selected tile is open - and any tile while the pointer is on it,
   which is where its pin and close are. */
.s:not(.on):not(:hover) :is(.m,.stl,.x){display:none}
.s:is(.on,:hover) .why{display:none}
.s .stl{font-size:12px;color:var(--muted);margin-top:2px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.s .stl.st-d-attn{color:var(--warn)}
.s .stl.st-d-fail{color:var(--fail)}
.s .stl.st-d-busy{color:var(--busy)}
.s .m .busy{color:var(--busy)}
.s .m .ctx-hi{color:var(--warn)}
.s .app{display:inline-block;width:13px;height:11px;margin-right:5px;vertical-align:-1px;
  color:var(--muted)}
.s .w{color:var(--muted);font-size:12px;margin-top:1px;font-style:italic;
  white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.s .m{color:var(--muted);font-size:12px;margin-top:2px;
  white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
/* A state is its colour and its shape, so it still reads without colour:
   working a filled dot, idle a ring, needs you a diamond, failed a cross,
   finished a dash. */
.dot{display:inline-block;width:8px;height:8px;border-radius:50%;margin-right:5px;
  position:relative;flex:none;background:var(--c)}
.d-idle{--c:var(--ok);background:transparent;box-shadow:inset 0 0 0 2px var(--c)}
.d-busy{--c:var(--busy)}
.d-attn{--c:var(--warn);border-radius:1.5px;transform:rotate(45deg) scale(.9)}
.d-done{--c:var(--done);height:2px;border-radius:1px;vertical-align:3px}
.d-fail{--c:var(--fail);background:transparent;border-radius:0}
.d-fail::before,.d-fail::after{content:"";position:absolute;left:-1px;right:-1px;top:3px;
  height:2px;border-radius:1px;background:var(--c);transform:rotate(45deg)}
.d-fail::after{transform:rotate(-45deg)}
.s.fail{border-left:3px solid var(--fail);padding-left:10px}

#main{flex:1;display:flex;flex-direction:column;min-width:0}
#head{padding:11px 16px;border-bottom:1px solid var(--line);background:var(--panel)}
#title{font-weight:600}
#title,#sub{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
#sub{color:var(--muted);font-size:12px}
#sessionpane{flex:1;display:flex;flex-direction:column;min-height:0;position:relative}
#log{flex:1;overflow-y:auto;padding:14px 16px;position:relative}
/* Show Most Recent and the cold-cache button: a zero-height strip stuck to
   the bottom of the log's view, so they float over the chat without taking
   a line of it. Show Most Recent only while the end of the conversation is
   out of view; the cold-cache button, above it, while the cache is cold -
   and the log keeps room under its end so it does not cover the last reply. */
#log .newest{position:sticky;bottom:0;height:0;pointer-events:none;z-index:2}
#log .newest .float{position:absolute;left:0;right:0;bottom:14px;display:flex;flex-direction:column;
  align-items:center;gap:8px}
#log .newest [data-newest],#log .newest .coldbtn{pointer-events:auto;box-shadow:0 2px 8px rgba(0,0,0,.25)}
#log .newest .cold:not([hidden]){align-self:stretch;display:flex;justify-content:center}
#log .newest [data-newest]{display:none}
#log.away .newest [data-newest]{display:block}
#log.cold{padding-bottom:150px}
.coldbtn{max-width:min(520px,calc(100% - 16px));background:var(--warnbg);border:1px solid var(--warn);
  border-radius:6px;overflow:hidden;font-size:12px}
.coldbtn .ct{padding:7px 12px;color:var(--ink)}
.coldbtn .ct b{color:var(--warn)}
.coldbtn .ct .cn{color:var(--muted);margin-top:3px}
.coldbtn .segs{display:flex;border-top:1px solid var(--warn)}
.coldbtn .segs button{flex:1;border-radius:0;background:transparent;color:var(--warn);font-weight:600;padding:7px 10px}
.coldbtn .segs button + button{border-left:1px solid var(--warn)}
.coldbtn .segs button:hover{background:var(--warn);color:var(--onaccent)}
/* What the session is waiting on, under the chat: a tab per shell or
   monitor (Auto first when there are several), sized for the tabs and four
   lines, dragged taller or shorter by its top edge. */
#procs{flex:none;display:flex;flex-direction:column;height:var(--ph,116px);min-height:64px;
  border-top:1px solid var(--line);background:var(--panel)}
#procgrip{flex:none;height:6px;margin-top:-3px;cursor:ns-resize;touch-action:none}
#procgrip:hover,#procs.drag #procgrip{background:var(--line)}
#proctabs{flex:none;display:flex;gap:2px;padding:0 10px;overflow-x:auto;scrollbar-width:thin}
#proctabs button{flex:none;max-width:260px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;
  background:transparent;border:0;border-bottom:2px solid transparent;border-radius:0;
  color:var(--muted);font-size:12px;padding:3px 8px}
#proctabs button:hover{color:var(--ink)}
#proctabs button.on{color:var(--ink);border-bottom-color:var(--busy)}
#proctabs button.live::after{content:"";display:inline-block;width:6px;height:6px;margin-left:5px;
  border-radius:50%;background:var(--busy);vertical-align:1px}
#proctabs .k{font:12px var(--mono);color:var(--busy);margin-right:5px}
#proctext{flex:1;min-height:0;margin:0;overflow:auto;padding:4px 16px 6px;
  font:12.5px/18px var(--mono);white-space:pre-wrap;word-break:break-word;color:var(--ink)}
#proctext .none{color:var(--muted);font-style:italic}
#shape{display:flex;flex-direction:column;gap:4px;padding:2px 9px 4px;font-size:12px;color:var(--muted)}
#shape label{display:flex;align-items:center;justify-content:space-between;gap:14px;white-space:nowrap}
#shape select.full{color:var(--muted)}
#shape select:not(.full){color:var(--ink);border-color:var(--busy)}
#jump{display:flex;gap:4px;flex:none}
#jump button{padding:2px 8px;font-size:12px;opacity:.75}
#jump button:hover{opacity:1}
details.cmp{margin:3px 0}
details.cmp>summary{cursor:pointer;list-style:none;white-space:nowrap;overflow:hidden;
  text-overflow:ellipsis;color:var(--muted);font-size:12.5px}
details.cmp>summary::before{content:"▸ ";font-size:10px}
details.cmp[open]>summary::before{content:"▾ "}
details.cmp[open]>summary .ln{display:none}
details.cmp>summary .k{font:12px var(--mono);color:var(--busy);margin-right:5px}
details.cmp>summary .k.err{color:var(--warn)}
.turn.flash{outline:2px solid var(--busy);outline-offset:4px;border-radius:2px}
.turn.cur-find{outline:2px solid var(--warn);outline-offset:4px;border-radius:2px}
#current{border-bottom:1px solid var(--line);background:var(--panel);padding:8px 16px;
  max-height:30vh;overflow-y:auto}
#current .hd{display:flex;justify-content:space-between;align-items:center;cursor:pointer;
  font-size:12px;text-transform:uppercase;letter-spacing:.06em;color:var(--muted)}
#current .bd{margin-top:5px}
#current.fold .bd{display:none}
details.compact{margin:0 0 15px;color:var(--muted)}
#log .earlier{text-align:center;margin:0 0 14px}
#log .earlier button{font-size:12px}
details.compact>summary{cursor:pointer;font-size:12px;text-transform:uppercase;
  letter-spacing:.06em;list-style:none;text-align:center;border-top:1px dashed var(--line);
  padding-top:6px}
details.compact[open]>summary{margin-bottom:6px}
button.tiny.on{color:var(--ink);border-color:var(--busy)}
#side h1 .ttl{display:flex;align-items:center;gap:6px}
#side h1 > span:last-child{white-space:nowrap}
button.tiny.icon{padding:2px 4px;line-height:0}
button.tiny.icon svg{fill:none;stroke:currentColor;stroke-width:1.3}
button.tiny.icon.on{color:var(--busy);border-color:var(--busy)}
button.tiny.icon.on svg{fill:var(--sel)}
/* Prose is held to a readable line (~110 characters); code, tables and tool
   output are not prose and may use the whole width. A short code block or
   result still lines up with the text, and a long one grows to the edge
   instead of scrolling sideways beside empty space. */
.turn{margin:0 0 15px;--prose:900px}
.turn .txt,.turn .md,.turn .h,.turn blockquote,.turn .think,.turn hr{max-width:var(--prose)}
.turn pre.code,.turn .res{width:fit-content;max-width:100%;min-width:min(100%,var(--prose))}
.who{font-size:12px;text-transform:uppercase;letter-spacing:.06em;
  color:var(--muted);margin-bottom:3px}
.who.you{color:var(--ink);font-weight:600}
.who .via{color:var(--muted);font-weight:400;font-size:12px}
.who.claude{color:var(--busy);font-weight:600}
.txt{white-space:pre-wrap;word-wrap:break-word}
.tbl{overflow-x:auto;margin:7px 0}
.tbl table{border-collapse:collapse;font-size:13px}
.tbl th,.tbl td{border:1px solid var(--line);padding:4px 9px;vertical-align:top;
  text-align:left}
.tbl th{background:var(--side);font-weight:600}
.tbl tbody tr:nth-child(even){background:var(--panel)}
.tbl code{background:var(--code);border-radius:3px;padding:0 3px;
  font:12px var(--mono)}
.txt code,.md code,.h code,blockquote code{background:var(--code);border-radius:3px;
  padding:0 3px;font:12.5px var(--mono)}
.txt a,.md a,.tbl a,.h a,blockquote a{color:var(--busy)}
.h{font-weight:600;margin:12px 0 4px}
.h1{font-size:18px}.h2{font-size:16px}.h3{font-size:14.5px}
.h4,.h5,.h6{font-size:13.5px;color:var(--muted)}
.md ul,.md ol{margin:4px 0;padding-left:22px}
.md li{margin:2px 0}
pre.code{background:var(--code);border-radius:5px;padding:8px 10px;margin:6px 0;
  font:12.5px/1.45 var(--mono);overflow-x:auto;white-space:pre}
blockquote{border-left:3px solid var(--line);margin:6px 0;padding:0 0 0 10px;
  color:var(--muted)}
#log hr{border:0;border-top:1px solid var(--line);margin:10px 0}
.tool{border-left:2px solid var(--busy);padding:3px 0 3px 9px;margin:7px 0;
  font:12.5px/1.5 var(--mono);color:var(--muted)}
.tool b{color:var(--busy);font-weight:600}
.res{background:var(--code);border-radius:5px;padding:7px 9px;margin:5px 0;
  font:12px/1.45 var(--mono);white-space:pre-wrap;
  max-height:220px;overflow:auto;word-break:break-all}
.res.err{border-left:2px solid var(--warn)}
.think{color:var(--muted);font-style:italic;white-space:pre-wrap;margin:5px 0}
.note{color:var(--muted);padding:26px 0;text-align:center}

#ask{padding:10px 16px;border-top:1px solid var(--line);background:var(--panel)}
#prompt{width:100%;background:var(--bg);color:var(--ink);border:1px solid var(--line);
  border-radius:6px;padding:8px 9px;font-size:14px;line-height:1.5;font-family:inherit;resize:vertical;min-height:62px}
#prompt:focus{outline:none;border-color:var(--busy)}
#bar{display:flex;align-items:center;gap:10px;margin-top:7px;flex-wrap:wrap}
button{background:var(--busy);color:var(--onaccent);border:0;border-radius:5px;
  padding:6px 13px;font-size:13px;font-family:inherit;cursor:pointer}
button.ghost{background:transparent;color:var(--muted);border:1px solid var(--line)}
button.rec{background:var(--warn);color:var(--onaccent);border-color:var(--warn)}
button:disabled{opacity:.45;cursor:default}
#say{color:var(--muted);font-size:12px}
#status{margin-top:8px;border-top:1px dashed var(--line);padding-top:5px;font-size:12px;
  color:var(--muted)}
#status .hd{display:flex;justify-content:space-between;gap:10px;cursor:pointer;user-select:none}
#status .hd .sum{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
#status.fold .bd{display:none}
#status .grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(190px,1fr));
  gap:2px 16px;margin-top:5px}
#status .c{white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
#status .c b{font-weight:400;margin-right:6px}
#status .c span{color:var(--ink)}
#status .c span.hot{color:var(--busy)}#status .c span.ok{color:var(--ok)}
#status .c span.warn{color:var(--warn)}
#status .ri{font:12px/1.5 var(--mono);white-space:nowrap;overflow:hidden;
  text-overflow:ellipsis}
#status .ri .k{color:var(--busy);margin-right:6px}
#status .run{margin-top:4px}

/* -- sidebar tools: filter, grouping, usage meter ---------------------- */
#tools{display:flex;gap:6px;padding:0 10px 8px;align-items:center}
#filter{flex:1;min-width:0;background:var(--panel);color:var(--ink);
  border:1px solid var(--line);border-radius:5px;padding:4px 7px;font-size:12.5px;font-family:inherit}
#filter:focus{outline:none;border-color:var(--busy)}
select.tiny{background:var(--panel);color:var(--muted);border:1px solid var(--line);
  border-radius:4px;font-size:12px;font-family:inherit;padding:2px 3px}
#meter{padding:0 12px 8px;position:relative;cursor:pointer}
.meters{font-size:12px;color:var(--muted)}
/* The rows share one grid - name, bar, percentage - so every bar is the
   same length whatever the name or the number. The percentage column is
   wide enough for "100%" from the start, so a reading going from 9% to 10%
   does not move the bars. When each window resets is in the Meter Tip,
   shown on hover. */
.meters .rows{display:grid;grid-template-columns:auto 1fr minmax(4ch,auto);column-gap:6px}
.meters .row{display:grid;grid-column:1/-1;grid-template-columns:subgrid;align-items:center;
  margin-top:3px;position:relative}
.meters .row .pct{text-align:right}
.meters .tip{display:none;position:absolute;left:0;right:0;top:calc(100% + 4px);z-index:30;
  padding:5px 8px;background:var(--panel);color:var(--ink);border:1px solid var(--line);
  border-radius:5px;box-shadow:0 4px 14px rgba(0,0,0,.22);line-height:1.45}
.meters .tip div:first-child{font-weight:600}
.meters .row:hover .tip{display:block}
.meters .bar{height:5px;border-radius:3px;background:var(--line);position:relative}
.meters .bar i{display:block;height:100%;border-radius:3px;background:var(--ok)}
.meters .bar b{position:absolute;top:-2px;bottom:-2px;width:2px;margin-left:-1px;background:var(--ink)}
.meters .bar i.hi{background:var(--warn)}.meters .bar i.over{background:var(--fail)}
.meters .pace{grid-column:2/-1;font-size:12px}
.meters .pace.bad{color:var(--fail)}
.meters .plan{margin-top:3px;color:var(--ink)}
/* Spend under the bars: Global (the account's windows, this machine) and
   Open (the sessions open now, whole sessions). Detail is in the title. */
.meters .spend{margin-top:5px;display:grid;grid-template-columns:auto 1fr;column-gap:8px;row-gap:1px}
.meters .spend .k{color:var(--muted)}
.meters .spend .v{color:var(--ink);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.meters .plan.also{color:var(--muted)}
/* Everything in the meter is one grid, so the model-mix bars line up with
   the meters. Lines that are not a row span it. */
.meters .rows>.plan,.meters .rows>.spend,.meters .cap,.meters .legend{grid-column:1/-1}
.meters .cap{margin-top:7px;text-transform:uppercase;letter-spacing:.06em}
.meters .stack{height:7px;border-radius:3px;background:var(--line);display:flex;gap:1px;overflow:hidden}
.meters .stack i,.meters .legend i{display:block;height:100%;background:var(--iron-other)}
.meters .i1{background:var(--iron-1)!important}.meters .i2{background:var(--iron-2)!important}
.meters .i3{background:var(--iron-3)!important}.meters .i4{background:var(--iron-4)!important}
.meters .legend{display:flex;flex-wrap:wrap;gap:2px 10px;margin-top:4px}
.meters .legend span{display:flex;align-items:center;gap:4px}
.meters .legend i{width:9px;height:9px;border-radius:2px}
/* The sidebar shows the bars alone; a click opens the rest (the Meter
   Details) and the caret turns. */
#meter:not(.open) .extra{display:none}
#meter::after{content:"\\25BE";position:absolute;right:1px;top:0;color:var(--muted);transition:transform .15s}
#meter.open::after{transform:rotate(180deg)}
#meter:empty::after{content:none}
#meter:focus-visible{outline:2px solid var(--busy);outline-offset:-2px}
.g{font-size:12px;text-transform:uppercase;letter-spacing:.07em;color:var(--muted);
  padding:8px 13px 3px;display:flex;justify-content:space-between;cursor:default}
.g.fold{cursor:pointer}
.fam{padding:7px 13px 4px;font-weight:600;font-size:12.5px;display:flex;align-items:center;
  gap:5px;border-bottom:1px solid var(--line);cursor:pointer;user-select:none}
.fam:hover{background:var(--panel)}
.fam .car{color:var(--muted);font-size:10px;width:9px}
.fam .dots{margin-left:auto;display:flex;gap:3px}
.fam .nowait{color:var(--muted);font-weight:400;font-size:12px}
.fam .dot{margin-right:0}
.kids{margin-left:13px;border-left:2px solid var(--line)}
.kids .s{padding-left:11px}
.s{position:relative}
.s .d{color:var(--ink);font-size:12px;margin-top:2px;opacity:.8;
  white-space:nowrap;overflow:hidden;text-overflow:ellipsis;
  font-family:var(--mono)}
.s .acts span{cursor:pointer;color:var(--muted);font-size:12px;padding:0 3px;
  border-radius:3px;background:var(--side)}
.s .acts span:hover{color:var(--ink)}
.s .pin{color:var(--warn);font-size:12px;margin-left:4px}
.badge{display:inline-block;font-size:12px;border-radius:3px;padding:0 4px;
  margin-left:4px;border:1px solid var(--line);color:var(--muted);font-style:normal}
.badge.passing{color:var(--ok);border-color:var(--ok)}
.badge.failing{color:var(--fail);border-color:var(--fail)}
.badge.pending{color:var(--busy);border-color:var(--busy)}
.badge.merged{color:#8a63d2;border-color:#8a63d2}
.badge.q{color:var(--busy);border-color:var(--busy)}
.badge.ctx-hi{color:var(--warn);border-color:var(--warn)}
.work{font-variant-numeric:tabular-nums}
.work.cache{color:var(--ok)}
.work.busy{color:var(--busy)}
#status .hd .work+.work{margin-left:6px}
.badge.work.cache{border-color:var(--ok)}
.badge.work.busy{border-color:var(--busy)}

/* -- main: tabs and panels ------------------------------------------- */
/* One row: the title on the left, every session control on the right -
   the Tab Picker, the New / Fork and View menus, find, the actions that stay
   buttons (between the two pairs of arrows, to keep them apart), prompt jump. The title gives way first; the controls wrap
   only when even a short title will not fit. */
#headrow{display:flex;justify-content:space-between;gap:12px;align-items:center}
#headtext{flex:1 1 140px;min-width:0}
#headctl{display:flex;align-items:center;gap:6px;flex-wrap:wrap;justify-content:flex-end;
  font-size:12px;color:var(--muted)}
#tabpick{color:var(--ink);font-size:12.5px;padding:2px 4px}
#acts{display:flex;gap:5px;flex-wrap:wrap;justify-content:flex-end}
details.menu{position:relative}
details.menu>summary{list-style:none;cursor:pointer;user-select:none;white-space:nowrap;
  border:1px solid var(--line);border-radius:4px;padding:2px 8px;font-size:12px;color:var(--muted)}
details.menu>summary::-webkit-details-marker{display:none}
details.menu>summary::after{content:" \\25BE"}
details.menu>summary:hover,details.menu[open]>summary{color:var(--ink)}
details.menu>summary.mod{color:var(--ink);border-color:var(--busy)}
details.menu .pop{position:absolute;right:0;top:calc(100% + 4px);z-index:20;min-width:190px;
  display:flex;flex-direction:column;padding:4px;background:var(--panel);
  border:1px solid var(--line);border-radius:6px;box-shadow:0 4px 14px rgba(0,0,0,.22)}
details.menu .pop>button{background:transparent;color:var(--ink);border:0;border-radius:4px;
  text-align:left;padding:5px 9px;font-size:12.5px;white-space:nowrap}
details.menu .pop>button:hover{background:var(--sel)}
details.menu .sep{border-top:1px solid var(--line);margin:4px 0}
/* Send's split half: the menu opens upward, the prompt box is at the bottom. */
.split{display:inline-flex;align-items:stretch}
.split>#send{border-radius:5px 0 0 5px}
.split>details.menu>summary{height:100%;box-sizing:border-box;padding:6px 7px;font-size:13px;
  border:0;border-left:1px solid rgba(0,0,0,.18);border-radius:0 5px 5px 0;
  background:var(--busy);color:var(--onaccent)}
.split>details.menu>summary::after{content:"\\25BE"}
.split>details.menu>summary:hover,.split>details.menu[open]>summary{color:var(--onaccent);filter:brightness(1.1)}
.split>details.menu.off>summary{opacity:.45;pointer-events:none}
.split>details.menu .pop{top:auto;bottom:calc(100% + 4px);left:0;right:auto}
details.menu .cap{padding:2px 9px;font-size:12px;text-transform:uppercase;letter-spacing:.06em;
  color:var(--muted)}
.panel{flex:1;overflow-y:auto;padding:14px 16px}
.files{font:12.5px/1.6 var(--mono);margin-bottom:10px}
.files .add{color:var(--ok)}.files .del{color:var(--fail)}
.files .code{display:inline-block;width:2.3em;color:var(--muted)}
.diff{font:12px/1.45 var(--mono);white-space:pre;overflow-x:auto;
  background:var(--code);border-radius:5px;padding:8px 10px}
.diff .a{color:var(--ok)}.diff .r{color:var(--fail)}.diff .h{color:var(--busy)}
.diff .f{font-weight:600;color:var(--ink)}
.diff .hh{color:var(--busy)}
.kv{color:var(--muted);font-size:12px;margin-bottom:8px}
.ev{font:12.5px/1.5 var(--mono);padding:2px 0;
  border-bottom:1px dashed var(--line);display:flex;gap:8px}
.ev .t{color:var(--muted);flex:none;width:5.2em}
.ev .ok{color:var(--ok)}.ev .bad{color:var(--fail)}.ev .run{color:var(--busy)}
.ev .x{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.ev .err{color:var(--fail);display:block;white-space:normal}
.card{border:1px solid var(--line);border-radius:6px;padding:9px 11px;margin:0 0 9px;
  background:var(--panel)}
.card h3{margin:0 0 4px;font-size:13.5px}
.card .btns{display:flex;gap:6px;margin-top:7px;flex-wrap:wrap}
.card small{color:var(--muted)}
button.sm{padding:3px 9px;font-size:12px}
button.danger{background:var(--fail)}
.hits{font-size:12.5px}
.hit{padding:8px 0;border-bottom:1px solid var(--line)}
.hit .tt{font-weight:600}
.hit .sn{color:var(--muted);font-size:12px;margin-top:2px}
.hit .btns{margin-top:5px;display:flex;gap:6px}
#hq{width:100%;background:var(--panel);color:var(--ink);border:1px solid var(--line);
  border-radius:6px;padding:7px 9px;font-size:14px;font-family:inherit;margin-bottom:10px}
table.t{border-collapse:collapse;font-size:12.5px;margin:4px 0 12px}
table.t th,table.t td{border-bottom:1px solid var(--line);padding:3px 10px 3px 0;text-align:left}
table.t td.n{text-align:right;font-variant-numeric:tabular-nums}
#queued{font-size:12px;color:var(--muted);margin-top:6px}
#queued .qi{display:flex;gap:6px;align-items:baseline}
#queued .qi span{flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
#queued .qi.err span{color:var(--fail)}
label.sw{display:flex;gap:7px;align-items:center;font-size:13px;margin:4px 0}
#decide{background:var(--warnbg);border-bottom:1px solid var(--warn);padding:8px 16px;
  font-size:13px}
#decide .dq{display:flex;gap:8px;align-items:center;margin:3px 0}
#decide .dq span{flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}

/* -- unread, warnings, jump palette, find, review, cost chart ------------ */
.badge.new{color:var(--busy);border-color:var(--busy);font-weight:600}
.s.unread .n{color:var(--busy)}
.fam .newn{color:var(--busy);font-weight:400;font-size:12px}
.badge.heavy{color:var(--warn);border-color:var(--warn)}
#heavy{font-size:12px;color:var(--warn);margin-top:6px}
#heavy div::before{content:"⚠ "}
#jumpbox,#quickbox{position:fixed;inset:0;background:rgba(0,0,0,.35);display:flex;justify-content:center;
  align-items:flex-start;padding-top:12vh;z-index:20}
#jumpbox .box,#quickbox .box{width:min(520px,92vw);background:var(--panel);border:1px solid var(--line);
  border-radius:8px;box-shadow:0 10px 30px rgba(0,0,0,.3);overflow:hidden}
#jq,#qq{width:100%;border:0;border-bottom:1px solid var(--line);background:var(--panel);color:var(--ink);
  padding:10px 12px;font-size:15px;outline:none}
#jlist,#qlist{max-height:50vh;overflow-y:auto}
#qlist .ji{display:flex;gap:8px;align-items:center;padding:6px 12px;cursor:pointer;font-size:13px}
#qlist .ji.on,#qlist .ji:hover{background:var(--sel)}
#qlist .qn{color:var(--muted);font-size:12px;width:1.2em;flex:none;text-align:right}
#qlist .note{padding:14px 12px;text-align:left}
#jlist .ji{display:flex;gap:8px;align-items:center;padding:6px 12px;cursor:pointer;font-size:13px}
#jlist .ji.on,#jlist .ji:hover{background:var(--sel)}
#jlist .ji .jn{font-weight:600;white-space:nowrap}
#jlist .ji .jw{color:var(--muted);font-size:12px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
#jumpbox .keys,#quickbox .keys{padding:6px 12px;font-size:12px;color:var(--muted);border-top:1px solid var(--line)}
#findbar{display:flex;align-items:center;gap:4px;flex:none}
#cfind{width:130px;transition:width .15s;background:var(--bg);color:var(--ink);border:1px solid var(--line);border-radius:4px;
  padding:2px 6px;font-size:12px}
#cfind:focus{outline:none;border-color:var(--busy)}
#cfcount{min-width:3.5em;text-align:right}
#cfcount:empty{display:none}
#cfind:focus,#cfind:not(:placeholder-shown){width:190px}
#findbar button{padding:2px 7px;font-size:12px}
#log mark{background:var(--warnbg);color:inherit;outline:1px solid var(--warn);border-radius:2px}
#log mark.cur{background:var(--warn);color:var(--onaccent)}
.dl{white-space:pre;min-height:1.45em}
.dl[data-r]{cursor:pointer}
.dl[data-r]:hover{background:var(--sel)}
.rv{font:13px/1.45 var(--ui);white-space:normal;background:var(--panel);border:1px solid var(--busy);
  border-radius:5px;margin:3px 0 5px;padding:5px 8px;max-width:760px}
.rv .rx{float:right;cursor:pointer;color:var(--muted)}
.rv textarea{width:100%;min-height:52px;background:var(--bg);color:var(--ink);border:1px solid var(--line);
  border-radius:4px;padding:4px 6px;font:13px var(--ui);resize:vertical}
.rv .btns{display:flex;gap:6px;margin-top:4px}
#review{border:1px solid var(--busy);border-radius:6px;padding:7px 10px;margin:0 0 10px;font-size:13px}
#review .btns{display:flex;gap:6px;margin-top:6px;flex-wrap:wrap}
#review .orph{color:var(--muted);font-size:12px}
.chart{width:100%;max-width:720px;height:auto;display:block;margin:4px 0 2px}
.chart .bar{fill:var(--busy)}
.chart .hit{fill:transparent}
.chart g:hover .bar{opacity:.7}
.chart .ax{stroke:var(--line)}
.chart text{fill:var(--muted);font:12px var(--ui)}

@media (max-width:760px){
  #app{flex-direction:column}
  #side{width:auto;max-height:38vh;border-right:0;border-bottom:1px solid var(--line)}
  #headrow{flex-direction:column}
}
"""

# What both pages need - the window and the chat matrix: naming a session's
# state, and rendering what Claude wrote.
SHARED = r"""
const esc = s => String(s == null ? "" : s)
  .replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;");

// A key event as a combination like "ctrl+space": modifiers in the order
// settings.normal_key writes them, then the physical key's name, so a
// layout or Shift does not change what a key is called. "" for a bare
// modifier press.
function comboOf(e){
  const c = e.code || "";
  if (/^(Control|Alt|Shift|Meta|OS)/.test(c)) return "";
  const key = /^Key[A-Z]$/.test(c) ? c.slice(3).toLowerCase()
            : /^Digit\d$/.test(c) ? c.slice(5)
            : c.toLowerCase();
  if (!key) return "";
  return [e.ctrlKey && "ctrl", e.altKey && "alt", e.shiftKey && "shift", e.metaKey && "win", key]
    .filter(Boolean).join("+");
}
// "ctrl+space" as people write it: "Ctrl+Space".
const prettyKey = k => String(k || "").split("+")
  .map(p => p.charAt(0).toUpperCase() + p.slice(1)).join("+");

// The caption now arrives as `attention_reason`, worded by the dispatcher
// from Claude Code's own table, so there is one vocabulary rather than one
// per surface. This map is only the fallback for a row recorded before that.
const WAITING = {
  permission_prompt: "needs permission",
  idle_prompt: "waiting for you",
  agent_needs_input: "needs input",
  elicitation_dialog: "needs an answer",
  elicitation_url_dialog: "needs a URL",
};

// The full set is ["busy","shell","idle","waiting"]; `shell` is a session
// running a command. Anything unhandled used to fall through to a grey dot
// captioned with its own enum name.
const RUNNING = {busy: "working", shell: "running a command"};

// The wire role says "user" for every tool result, because that is how a
// result is handed back to the model - not who wrote it. The server sends a
// `speaker` derived from content; this only has to name it.
// `system` is Claude Code itself (a task finishing, a command's output); the
// item's `note` says which.
const SPEAKER = {you: "you", claude: "Claude", tool: "tool result", system: "Claude Code"};

// -- spend ------------------------------------------------------------------
// Global is everything sent from this machine in the account's windows (5h
// and 7d on a subscription, the month otherwise); current is the sessions
// open now, each over its whole life. Both are estimates at API list prices.
const SPAN_NAMES = {five_hour: "5h", seven_day: "7d", month: "month"};
const SPAN_TITLES = {five_hour: "5-hour window", seven_day: "7-day window", month: "This month"};
const usd = v => typeof v !== "number" ? "" : "$" + (v >= 100 ? Math.round(v) : v.toFixed(2));
function tok(n){
  n = n || 0;
  if (n >= 1e9) return (n / 1e9).toFixed(1) + "B";
  if (n >= 1e6) return (n / 1e6).toFixed(n >= 1e7 ? 0 : 1) + "M";
  if (n >= 1e3) return (n / 1e3).toFixed(n >= 1e4 ? 0 : 1) + "k";
  return String(Math.round(n));
}
const hhmm = t => new Date(t * 1000).toLocaleTimeString([], {hour: "2-digit", minute: "2-digit"});
// One block of spend, spelled out for a tooltip.
function spendDetail(x){
  const t = x.tokens || {};
  return `${tok(x.total_tokens)} tokens: in ${tok(t.input)}, out ${tok(t.output)}, ` +
    `cache read ${tok(t.cache_read)}, cache write ${tok(t.cache_write)} · ≈${usd(x.cost_usd || 0)}` +
    (x.unpriced_tokens ? ` (${tok(x.unpriced_tokens)} tokens on models with no known price)` : "");
}
function windowTitle(w){
  const since = w.since ? (w.name === "month" ? "" : ` since ${hhmm(w.since)}`) : "";
  return (SPAN_TITLES[w.name] || w.name) + (w.resets_at ? `${since}, resets ${hhmm(w.resets_at)}`
    : w.name === "month" ? "" : " (the last " + (w.name === "five_hour" ? "5 hours" : "7 days") + ", reset time not known yet)");
}
const SPEND_NOTE = "Estimated at API list prices from the transcripts on this machine; " +
  "usage elsewhere (claude.ai, other machines) shows only in the meters' percentages.";

// -- usage meter ---------------------------------------------------------
const clock = t => new Date(t * 1000).toLocaleTimeString([], {hour:"2-digit", minute:"2-digit"});
const LIMIT_NAMES = {five_hour: "5h", seven_day: "7d", spend_limit: "spend",
                     seven_day_opus: "7d opus", seven_day_sonnet: "7d sonnet"};
const LIMIT_TITLES = {five_hour: "5-hour window", seven_day: "7-day window", spend_limit: "Spend limit",
                      seven_day_opus: "7-day Opus window", seven_day_sonnet: "7-day Sonnet window"};
const LIMIT_SECONDS = {five_hour: 5 * 3600, seven_day: 7 * 86400};
// A reset that isn't today names its weekday too: "Tue 09:00".
const resetLabel = t => {
  const d = new Date(t * 1000);
  return (d.toDateString() === new Date().toDateString() ? ""
          : d.toLocaleDateString([], {weekday: "short"}) + " ") + clock(t);
};
// The tick: how far through the window the clock is, so the fill can be read
// against time - fill ahead of the tick means using faster than the window allows.
const timeTick = (name, resets) => {
  const len = LIMIT_SECONDS[name];
  if (!len || !resets) return "";
  const done = Math.max(0, Math.min(1, 1 - (resets - Date.now() / 1000) / len));
  return `<b style="left:${(done * 100).toFixed(1)}%"></b>`;
};
// "2h 5m", "3d 4h": how long until a reset, for the Meter Tip.
const until = t => {
  const m = Math.max(0, Math.round((t - Date.now() / 1000) / 60));
  if (m < 60) return m + "m";
  const h = Math.floor(m / 60);
  return h < 24 ? h + "h" + (m % 60 ? " " + (m % 60) + "m" : "") : Math.floor(h / 24) + "d " + (h % 24) + "h";
};
// The Meter Tip: what a row's numbers mean and when its window resets.
const meterTip = (name, w, spent) => {
  if (w.waiting) {
    // The window reset and no session has reported the new one yet.
    const lines = [esc(LIMIT_TITLES[name] || name), "reset at " + esc(resetLabel(w.reset_at)) +
      ", waiting for a new reading", "shows 0% until a session reports the new window"];
    if (spent) lines.push("from this machine: " + tok(spent.total_tokens) + " tokens · ≈" + usd(spent.cost_usd || 0));
    return `<div class="tip">${lines.map(l => `<div>${l}</div>`).join("")}</div>`;
  }
  const lines = [esc(LIMIT_TITLES[name] || name), Math.round(w.used) + "% used" +
    // Kept across restarts, so it can be from before this dispatcher started.
    (w.at && Date.now() / 1000 - w.at > 600 ? ` (reading from ${esc(resetLabel(w.at))})` : "")];
  if (w.resets_at) lines.push("resets " + esc(resetLabel(w.resets_at)) + " (in " + until(w.resets_at) + ")");
  const len = LIMIT_SECONDS[name];
  if (len && w.resets_at)
    lines.push(Math.round(Math.max(0, Math.min(1, 1 - (w.resets_at - Date.now() / 1000) / len)) * 100)
               + "% of the window elapsed (the tick)");
  if (spent) lines.push("from this machine: " + tok(spent.total_tokens) + " tokens · ≈" + usd(spent.cost_usd || 0));
  return `<div class="tip">${lines.map(l => `<div>${l}</div>`).join("")}</div>`;
};

// Global and Open lines under the bars.
function spendHtml(sp){
  if (!sp || !sp.global) return "";
  const wins = sp.global.windows || [];
  const more = sp.complete === false ? " · counting…" : "";
  const g = wins.map(w => `${SPAN_NAMES[w.name] || w.name} ≈${usd(w.cost_usd || 0)}`).join(" · ");
  const gt = wins.map(w => windowTitle(w) + ": " + spendDetail(w)).join("\n") + "\n\n" + SPEND_NOTE;
  const c = sp.current || {};
  const ct = "The sessions open in Imperatorium, each over its whole life, subagents included:\n" +
    (c.each || []).map(e => `${e.name || e.sessionId}: ${tok(e.total_tokens)} tokens · ≈${usd(e.cost_usd || 0)}`).join("\n") +
    "\n\nTotal " + spendDetail(c) + "\n\n" + SPEND_NOTE;
  return `<div class="spend extra">` +
    (wins.length ? `<span class="k">global</span><span class="v" title="${esc(gt)}">${esc(g + more)}</span>` : "") +
    `<span class="k">open</span><span class="v" title="${esc(ct)}">${
      c.sessions || 0} session${c.sessions === 1 ? "" : "s"} · ≈${usd(c.cost_usd || 0)} · ${tok(c.total_tokens)} tokens</span></div>`;
}

// -- model mix ---------------------------------------------------------
// After claude-lens's "Model mix" chart: model families in cost order,
// cheapest first, which is both the stacking order (left to right here) and
// the rank on the ironbow ramp, so a bar reads as a cost gradient. It stacks
// output tokens. "other" is a model with no known price, which is not the
// same as an expensive one, so it is grey. "other" catches all: keep it last.
const FAMILIES = [
  ["haiku",  m => m.includes("haiku"),  "i1"],
  ["sonnet", m => m.includes("sonnet"), "i2"],
  ["opus",   m => m.includes("opus"),   "i3"],
  ["fable",  m => m.includes("fable") || m.includes("mythos"), "i4"],
  ["other",  () => true, "io"],
];
const famOf = model => FAMILIES.find(([, test]) => test(String(model || "").toLowerCase()))[0];
// {family: {output, total_tokens, cost_usd}} from the server's split by model.
function byFamily(models){
  const out = {};
  for (const [name, m] of Object.entries(models || {})) {
    const k = famOf(name), f = out[k] || (out[k] = {output: 0, total_tokens: 0, cost_usd: 0});
    f.output += m.output || 0; f.total_tokens += m.total_tokens || 0; f.cost_usd += m.cost_usd || 0;
  }
  return out;
}
function mixRow(label, title, models){
  const fams = byFamily(models);
  const have = FAMILIES.filter(([n]) => fams[n]);
  const total = have.reduce((a, [n]) => a + fams[n].output, 0);
  const segs = have.filter(([n]) => fams[n].output > 0).map(([n, , slot]) =>
    `<i class="${slot}" style="width:${(100 * fams[n].output / total).toFixed(2)}%"></i>`).join("");
  const lines = [esc(title) + ": output tokens by model"].concat(have.map(([n]) => {
    const f = fams[n];
    return `${n}: ${tok(f.output)} out (${total ? Math.round(100 * f.output / total) : 0}%) · ` +
      `${tok(f.total_tokens)} tokens in all · ≈${usd(f.cost_usd)}`;
  }));
  if (!have.length) lines.push("nothing sent");
  return `<div class="row mix extra"><span>${esc(label)}</span><span class="stack">${segs}</span>` +
    `<span class="pct">${tok(total)}</span><div class="tip">${lines.map(l => `<div>${l}</div>`).join("")}</div></div>`;
}
// One row per global window, then the open sessions; a legend of the
// families that appear.
function mixHtml(sp){
  if (!sp || !sp.global) return "";
  const wins = sp.global.windows || [], cur = sp.current;
  const rows = wins.map(w => mixRow(SPAN_NAMES[w.name] || w.name, SPAN_TITLES[w.name] || w.name, w.models));
  if (cur) rows.push(mixRow("open", "Open sessions", cur.models));
  if (!rows.length) return "";
  const seen = new Set();
  for (const m of wins.map(w => w.models).concat(cur ? [cur.models] : []))
    for (const n of Object.keys(m || {})) seen.add(famOf(n));
  const legend = FAMILIES.filter(([n]) => seen.has(n))
    .map(([n, , slot]) => `<span><i class="${slot}"></i>${n}</span>`).join("");
  return `<div class="cap extra">model mix · output tokens</div>` + rows.join("") +
    (legend ? `<div class="legend extra">${legend}</div>` : "");
}


// No windows at all is normal off a Pro or Max plan: Bedrock, Vertex, an API
// key, or a Team or Enterprise seat has no 5h/7d limits to show, so the
// space names the plan instead of sitting empty like a broken meter.
function planHtml(plan){
  if (!plan || !plan.label) return "";
  return plan.alongside
    ? `<div class="plan also" title="sessions on these have no rate-limit windows">also ${esc(plan.label)} - no limits</div>`
    : `<div class="plan" title="no rate-limit windows on this plan">${esc(plan.label)}</div>`;
}

// The meters: a row per window, then (marked extra, so the sidebar shows
// them only when opened) the pace lines, the plan, the spend lines and the
// model mix. `lines: false` leaves out the spend lines, for the matrix's
// status tile, which spells the spend out below.
function meterHtml(u, opts){
  const lines = !opts || opts.lines !== false;
  const rows = Object.entries(u.limits || {});
  const spent = {};
  for (const w of ((u.spend || {}).global || {}).windows || []) spent[w.name] = w;
  const bars = rows.map(([name, w]) => {
    const pct = Math.max(0, Math.min(100, w.used));
    const cls = w.used >= 95 ? "over" : w.used >= 80 ? "hi" : "";
    const p = w.pace;
    let pace = "";
    if (p && p.full_at && p.before_reset)
      pace = `<div class="pace bad extra">at this pace: full by ${clock(p.full_at)} (${Math.round(p.per_hour)}%/h)</div>`;
    else if (p && p.per_hour > 0)
      pace = `<div class="pace extra">${Math.round(p.per_hour)}%/h - lasts until reset</div>`;
    return `<div class="row${w.waiting ? " waiting" : ""}"><span>${esc(LIMIT_NAMES[name] || name)}</span>
      <span class="bar"><i class="${cls}" style="width:${pct}%"></i>${timeTick(name, w.resets_at)}</span>
      <span class="pct">${Math.round(w.used)}%</span>${meterTip(name, w, spent[name])}</div>${pace}`;
  }).join("");
  const rest = (lines ? spendHtml(u.spend) : "") + mixHtml(u.spend);
  if (!bars && !rest) return planHtml(u.plan);
  return `<div class="rows">` + bars + planHtml(u.plan) + rest + `</div>`;
}

function stateOf(s){
  if (s.attention) return [s.attention_kind === "failure" ? "d-fail" : "d-attn",
    s.attention_reason || s.waitingFor ||
    WAITING[s.last_notification] || s.last_notification || "waiting for you"];
  if (s.state && s.state !== "working") return ["d-done", s.state];
  if (RUNNING[s.status] || s.state === "working")
    return ["d-busy", RUNNING[s.status] || "working"];
  // Idle between turns but a command it started is still running: it will wake
  // when that exits, so it is not free for more work yet.
  if (s.status === "idle" && s.background_tasks)
    return ["d-busy", s.background_tasks === 1 ? "waiting on a background task"
                      : `waiting on ${s.background_tasks} background tasks`];
  if (s.status === "idle") return ["d-idle","idle"];
  return ["d-done", s.status || "unknown"];
}

// Markdown tables, the GFM shape Claude writes: a header row, a delimiter row
// of dashes (colons set alignment), then body rows until a line without a
// pipe. Nothing inside a ``` fence is touched - a table in a code sample is
// meant to be read as source.
const TABLE_SEP = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;

function cells(line){
  let s = line.trim();
  if (s.startsWith("|")) s = s.slice(1);
  if (s.endsWith("|") && !s.endsWith("\\|")) s = s.slice(0, -1);
  return s.split(/(?<!\\)\|/).map(c => c.trim().replace(/\\\|/g, "|"));
}

// Inline Markdown: `code`, [links](https://...), bare URLs, **bold**,
// *italic*, ~~strike~~. Code and links are set aside first so nothing inside
// them is read as emphasis; underscores only count at word edges, so
// snake_case names and paths come through untouched. Only http(s) links are
// made clickable - a session's output is not trusted to pick a scheme.
const attr = s => esc(s).replace(/"/g, "&quot;");
const link = (href, text) => `<a href="${attr(href)}" target="_blank" rel="noopener">${text}</a>`;

function emph(s){
  return s
    .replace(/\*\*(?!\s)([^\n]+?)(?<!\s)\*\*/g, "<b>$1</b>")
    .replace(/(^|[^\w])__(?!\s)([^\n]+?)(?<!\s)__(?!\w)/g, "$1<b>$2</b>")
    .replace(/(^|[^*\w])\*(?![\s*])([^*\n]+?)(?<!\s)\*(?![*\w])/g, "$1<i>$2</i>")
    .replace(/(^|[^\w])_(?![\s_])([^_\n]+?)(?<!\s)_(?!\w)/g, "$1<i>$2</i>")
    .replace(/~~(?!\s)([^\n]+?)(?<!\s)~~/g, "<s>$1</s>");
}

function inline(s){
  const kept = [];
  const hold = h => "\u0000" + (kept.push(h) - 1) + "\u0000";
  s = String(s == null ? "" : s)
    .replace(/`([^`\n]+)`/g, (_, c) => hold("<code>" + esc(c) + "</code>"))
    .replace(/\[([^\]\n]+)\]\((https?:\/\/[^\s)]+)\)/g, (_, t, u) => hold(link(u, emph(esc(t)))))
    .replace(/\bhttps?:\/\/[^\s<>()\[\]"'`\u0000]*[^\s<>()\[\]"'`\u0000.,;:!?*_~]/g,
             u => hold(link(u, esc(u))));
  return emph(esc(s)).replace(/\u0000(\d+)\u0000/g, (_, i) => kept[+i]);
}

function renderTable(head, sep, rows){
  const align = cells(sep).map(c =>
    c.startsWith(":") && c.endsWith(":") ? "center" : c.endsWith(":") ? "right" :
    c.startsWith(":") ? "left" : "");
  const row = (tag, cs) => "<tr>" + align.map((a, i) =>
    `<${tag}${a ? ` style="text-align:${a}"` : ""}>${inline(cs[i] || "")}</${tag}>`
  ).join("") + "</tr>";
  return `<div class="tbl"><table><thead>${row("th", head)}</thead><tbody>${
    rows.map(r => row("td", r)).join("")}</tbody></table></div>`;
}

// Block Markdown, the subset Claude actually writes: fenced code, headings,
// bullet and numbered lists (nested by indent), block quotes, rules and
// tables. Prose between them stays one pre-wrapped run per paragraph group,
// so line breaks Claude put there survive as they always have.
const FENCE = /^\s*(```|~~~)/;
const HEADING = /^(#{1,6})\s+(.*?)\s*#*\s*$/;
const ITEM = /^(\s*)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const RULE = /^\s*([-*_])(\s*\1){2,}\s*$/;
const QUOTE = /^\s*>\s?(.*)$/;
const indentOf = s => s.replace(/\t/g, "    ").length;

function renderText(text){
  const lines = String(text == null ? "" : text).split("\n");
  const out = [];
  let run = [], lists = [], listHtml = "";
  const flush = () => {
    const t = run.join("\n").replace(/^\n+|\n+$/g, "");
    if (t) out.push(`<div class="txt">${inline(t)}</div>`);
    run = [];
  };
  const closeList = () => {
    const top = lists.pop();
    listHtml += (top.open ? "</li>" : "") + (top.ordered ? "</ol>" : "</ul>");
  };
  const endLists = () => {
    while (lists.length) closeList();
    if (listHtml) out.push(`<div class="md">${listHtml}</div>`);
    listHtml = "";
  };
  const block = html => { flush(); endLists(); out.push(html); };

  for (let i = 0; i < lines.length; i++){
    const line = lines[i];
    const next = lines[i + 1];

    if (FENCE.test(line)){
      const mark = line.trim().slice(0, 3), body = [];
      let j = i + 1;
      while (j < lines.length && !lines[j].trim().startsWith(mark)) body.push(lines[j++]);
      block(`<pre class="code">${esc(body.join("\n"))}</pre>`);
      i = j;
      continue;
    }
    if (line.includes("|") && next !== undefined && TABLE_SEP.test(next) && next.includes("|")){
      const head = cells(line);
      if (cells(next).length === head.length){
        const rows = [];
        let j = i + 2;
        while (j < lines.length && lines[j].includes("|") && lines[j].trim())
          rows.push(cells(lines[j++]));
        block(renderTable(head, next, rows));
        i = j - 1;
        continue;
      }
    }
    const item = ITEM.exec(line);
    if (item && !RULE.test(line)){
      flush();
      const indent = indentOf(item[1]), ordered = /\d/.test(item[2]);
      while (lists.length && indent < lists[lists.length - 1].indent) closeList();
      let top = lists[lists.length - 1];
      if (top && indent === top.indent && top.ordered !== ordered){ closeList(); top = null; }
      else if (top && indent < top.indent) top = null;
      if (!top || indent > top.indent){
        const n = parseInt(item[2], 10);
        listHtml += ordered ? (n > 1 ? `<ol start="${n}">` : "<ol>") : "<ul>";
        lists.push(top = {indent, ordered, open: false});
      }
      listHtml += (top.open ? "</li>" : "") + "<li>" + inline(item[3]);
      top.open = true;
      continue;
    }
    if (lists.length){
      // A list survives a blank line when it carries on after it, and takes
      // indented lines as the current item's continuation.
      if (!line.trim()){
        const after = lines.slice(i + 1).find(l => l.trim());
        if (after !== undefined && (ITEM.test(after) || /^\s{2,}/.test(after))) continue;
      } else if (/^\s{2,}/.test(line)){
        listHtml += "<br>" + inline(line.trim());
        continue;
      }
      endLists();
    }
    const h = HEADING.exec(line);
    if (h){ block(`<div class="h h${h[1].length}">${inline(h[2])}</div>`); continue; }
    if (RULE.test(line) && line.trim()){ block("<hr>"); continue; }
    if (QUOTE.test(line)){
      const inner = [];
      let j = i;
      while (j < lines.length && QUOTE.test(lines[j])) inner.push(QUOTE.exec(lines[j++])[1]);
      block(`<blockquote>${renderText(inner.join("\n"))}</blockquote>`);
      i = j - 1;
      continue;
    }
    run.push(line);
  }
  flush();
  endLists();
  return out.join("");
}

"""

SCRIPT = SHARED + r"""
const $ = s => document.querySelector(s);
let sessions = [], selected = null, lastSig = "", busy = false;

function age(t){
  if (!t) return "";
  const d = Math.max(0, Math.floor(Date.now()/1000 - t));
  if (d < 60) return d + "s";
  if (d < 3600) return Math.floor(d/60) + "m";
  return Math.floor(d/3600) + "h";
}

async function get(url){
  const r = await fetch(url, {cache:"no-store"});
  if (!r.ok) throw new Error(r.status);
  return r.json();
}

// Served with the page and never stored: what Send As Typed asks for.
const windowToken = () => (document.querySelector("meta[name=imp-window]") || {}).content || "";
async function post(url, body){
  const r = await fetch(url, {method:"POST", cache:"no-store",
    headers:{"Content-Type":"application/json", "X-Imperatorium-Window": windowToken()},
    body:JSON.stringify(body)});
  return r.json();
}

function key(s){ return s.sessionId || s.id; }
// The store's key: host-qualified for a remote session. Pins and archive
// are kept against this so a local and a remote session never share one.
function pkey(s){ return s.key || key(s); }

// -- sidebar preferences -------------------------------------------------
// Kept by the dispatcher, not the browser, so a tablet sees the same pins.
let prefs = {pinned: [], archived: [], group: "state"};
let showArchived = false;

async function savePrefs(change){
  Object.assign(prefs, change);
  try { const r = await post("/api/prefs", change); if (r.prefs) Object.assign(prefs, r.prefs); }
  catch (e) {}
  drawSessions();
}
function toggleIn(list, k){
  const l = (prefs[list] || []).slice();
  const i = l.indexOf(k);
  if (i >= 0) l.splice(i, 1); else l.push(k);
  return l;
}

const folder = cwd => String(cwd || "").replace(/[\\/]+$/, "").split(/[\\/]/).pop() || "(no folder)";

// What a tile calls a session. The server turns a worktree session's
// made-up "<worktree>-<id>" into "<project>-<worktree>"; a chosen name stays.
const nameOf = s => s.display_name || s.name || s.id || "(unnamed)";
// The project a session belongs to - for a worktree, the repository it was
// made from, not the worktree's folder.
const projectOf = s => (s.project || folder(s.cwd)) + (s.host ? " @ " + s.host : "");

function groupOf(s){
  if (prefs.group === "folder") return projectOf(s);
  if (prefs.group === "state"){
    if (s.attention) return "needs you";
    const dot = stateOf(s)[0];
    if (dot === "d-busy") return "working";
    if (dot === "d-idle") return "idle";
    return "done";
  }
  return "";
}
const STATE_ORDER = ["needs you", "working", "idle", "done"];

function prBadge(pr){
  if (!pr) return "";
  const cls = pr.state === "merged" ? "merged" : pr.state === "closed" ? "" : pr.checks;
  const mark = {passing:"✓", failing:"✗", pending:"…"}[pr.checks] || "";
  const what = pr.state === "open" ? (pr.draft ? "draft" : "") : pr.state;
  return `<span class="badge pr ${esc(cls)}" data-url="${esc(pr.url)}"
    title="${esc(pr.title)}${pr.review ? " · review: " + esc(pr.review) : ""}${
    " · checks: " + esc(pr.checks)}">#${esc(pr.number)}${mark ? " " + mark : ""}${
    what ? " " + esc(what) : ""}</span>`;
}

// Marks a session hosted by the Claude desktop app: it lives in that app's
// window, so someone scanning terminals for it would never find it.
const APP_ICON = `<svg class="app" viewBox="0 0 13 11" aria-label="Claude desktop app"><title>in the Claude desktop app</title><rect x=".5" y=".5" width="12" height="10" rx="1.5" fill="none" stroke="currentColor"/><path d="M.5 3.5h12" stroke="currentColor"/></svg>`;
// Marks a session working in a worktree rather than the main checkout.
const WT_ICON = `<svg class="app" viewBox="0 0 11 13" aria-label="worktree"><title>in a worktree</title><circle cx="3" cy="2.5" r="1.6" fill="none" stroke="currentColor"/><circle cx="3" cy="10.5" r="1.6" fill="none" stroke="currentColor"/><circle cx="8.5" cy="4.5" r="1.6" fill="none" stroke="currentColor"/><path d="M3 4.1v4.8M8.5 6.1c0 2.2-5.5 1.4-5.5 2.8" fill="none" stroke="currentColor"/></svg>`;

// Pure: the prompt cache's countdown for a tile, "-4" minutes until it
// expires. A closed tile shows it only in the last CACHE_SOON seconds, when
// it is worth acting on; `all` (the open tile) shows it whenever it is warm.
// Nothing once it has expired - the cold-cache badge says that instead.
const CACHE_SOON = 600;
function workParts(w, now, all){
  if (!w) return [];
  const left = w.cache_expires_at ? w.cache_expires_at - now : 0;
  return left > 0 && (all || left <= CACHE_SOON) ? [{text: "-" + Math.ceil(left / 60), cls: "cache",
    title: `prompt cache expires in ${Math.ceil(left / 60)} min; a prompt after that re-sends the whole context`}] : [];
}
const workHtml = (w, now, cls, all) => workParts(w, now, all).map(p =>
  `<span class="${cls} ${p.cls}" title="${esc(p.title)}">${esc(p.text)}</span>`).join("");

// Pure: what the open tile adds under its first two lines - how long it has
// been idle (or working), its context size, what it has cost so far (from
// the status panel's answer, `spend`), and what it still has running while
// its process lives.
function tileDetails(s, now, spend){
  const out = [], w = s.work || {}, u = s.usage || {};
  const idle = s.status === "idle" && s.status_changed_at;
  if (idle) out.push({text: "idle " + span(now - idle), title: "idle since its last turn ended"});
  else {
    const at = s.status_changed_at || s.last_event_at;
    if (at) out.push({text: (stateOf(s)[0] === "d-busy" ? "working " : "active ") + span(now - at),
      title: "in this state since"});
  }
  if (w.context_tokens || u.context_pct != null)
    out.push({text: "ctx " + [w.context_tokens ? kfmt(w.context_tokens) : "",
      u.context_pct != null ? "(" + Math.round(u.context_pct) + "%)" : ""].filter(Boolean).join(" "),
      title: "context size", cls: u.context_pct >= CTX_HI ? "ctx-hi" : ""});
  if (spend && spend.cost_usd != null)
    out.push({text: "≈" + usd(spend.cost_usd), title: "the whole session so far, subagents included, at API list prices"});
  if (w.alive){
    const n = (w.agents || 0) + (w.processes || 0);
    if (n) out.push({text: n + " running", cls: "busy",
      title: `${w.agents || 0} subagents, ${w.processes || 0} shell processes ` +
        `(${w.shells || 0} background shells and ${w.monitors || 0} monitors among them)`});
  }
  return out;
}

// Pure: the closed tile's right-hand signals, each only while it is out of
// the ordinary and always in this order, so the eye learns where to look.
// How long it has waited is the open tile's "idle" figure, not said here.
const CTX_HI = 80;
function tileSignals(s, now, open){
  const u = s.usage || {}, out = [];
  if (isUnread(s)) out.push(`<span class="badge new" title="finished a turn since you last looked at it">new</span>`);
  out.push(workHtml(s.work, now, "badge work", open));
  heavyWarnings(s, now).filter(h => h.kind === "cold").forEach(h =>
    out.push(`<span class="badge heavy" title="${esc(h.text)}">${esc(h.short)}</span>`));
  if (!open && u.context_pct >= CTX_HI)
    out.push(`<span class="badge ctx-hi" title="context used">ctx ${Math.round(u.context_pct)}%</span>`);
  if (s.queued) out.push(`<span class="badge q" title="prompts queued">${s.queued} queued</span>`);
  if (!open && s.pr && s.pr.checks === "failing" && s.pr.state === "open") out.push(prBadge(s.pr));
  return out.join("");
}

// A closed tile is two lines however little it knows: its dot, name and why
// it is in its state (only when the dot does not already say it), its
// signals on the right; then its window title, or what it is doing, or its
// folder. Open - the selected tile, and whichever one the pointer is over
// (CSS) - the reason moves to a line of its own, the window title and what
// it is doing get a line each, and tileDetails with its pin and close
// follows. Every line is drawn; CSS shows the open ones. The signals are
// the open tile's for the selected one only.
// `short` is for a tile under its project's node, where the project's name
// would only be said twice; the chat header still carries the full name.
function tile(s, short){
  const [dot, label] = stateOf(s);
  const now = Date.now() / 1000, open = key(s) === selected;
  const mark = s.attention ? (s.attention_kind === "failure" ? " fail" : " attn") : "";
  const pinned = (prefs.pinned || []).includes(pkey(s));
  const archived = (prefs.archived || []).includes(pkey(s));
  const plain = dot === "d-idle" || label === "working";
  // The doing line is extra ("x", open only) when a window title is there
  // to be the closed tile's second line.
  const title = s.window_title ? `<div class="w" title="window title">${esc(s.window_title)}</div>` : "";
  const doing = s.doing ? `<div class="d${title ? " x" : ""}" title="${esc(s.doing)}">${esc(s.doing)}${
      s.doing_since ? " · " + age(s.doing_since) : ""}</div>` : "";
  const lines = title + doing || `<div class="w" title="${esc(s.cwd || "")}">${esc(s.cwd || "")}</div>`;
  const acts = `<span data-pin="${esc(pkey(s))}" title="${pinned?"unpin":"pin to top"}">${
      pinned ? "★" : "☆"}</span>${archived ? `<span data-arch="${esc(pkey(s))}" title="unarchive">↩</span>`
      : s.pid && !s.remote ? `<span data-close="${esc(key(s))}" title="close this session and its window (the conversation stays resumable)">✕</span>` : ""}`;
  const spend = open && statusFor === key(s) && statusData ? statusData.spend : null;
  const details = `<div class="m"><span class="det">${tileDetails(s, now, spend).map(p =>
    `<span class="${p.cls || ""}" title="${esc(p.title)}">${esc(p.text)}</span>`).join(" · ")}${
    s.host ? " · " + esc(s.host) : ""}${prBadge(s.pr)}</span><span class="acts">${acts}</span></div>`;
  return `<div class="s${open ? " on" : ""}${mark}${isUnread(s) ? " unread" : ""}" data-k="${esc(key(s))}">
    <div class="t1"><span class="dot ${dot}" title="${esc(label)}"></span><span class="n" title="${esc(nameOf(s))}">${
      s.desktop_app ? APP_ICON : ""}${s.worktree ? WT_ICON : ""}${esc(short && s.short_name || nameOf(s))}${
      pinned ? `<span class="pin">★</span>` : ""}</span>${
      plain ? "" : `<span class="why" title="${esc(label)}">${esc(label)}</span>`}<span class="sig">${
      tileSignals(s, now, open)}</span></div><div class="stl st-${dot}" title="${esc(label)}">${
      esc(label)}</div>${lines}${details}
  </div>`;
}

// Tiles for one group, with a project's sessions gathered under a node named
// for the project whenever it has more than one. The node carries one dot per
// session, so how many there are and what each is doing still shows when it
// is folded. A click on the node hides or shows all of it; a double click
// hides or shows only the sessions waiting for you, leaving the rest.
// archive does. Grouped by folder, the whole list is that tree: every
// project gets a node, one session or several, in place of a group header.
function tree(list){
  const fams = new Map();
  for (const s of list){
    const f = projectOf(s);
    if (!fams.has(f)) fams.set(f, []);
    fams.get(f).push(s);
  }
  return [...fams].map(([f, kids]) => {
    // A project with one session needs no header, in any grouping.
    if (kids.length < 2) return kids.map(s => tile(s)).join("");
    const shut = folded.has(f), noWait = waitHidden.has(f);
    const dots = kids.map(s => `<span class="dot ${stateOf(s)[0]}" title="${
      esc(nameOf(s) + ": " + stateOf(s)[1])}"></span>`).join("");
    const shown = shut ? [] : kids.filter(s => !(noWait && s.attention));
    const nNew = kids.filter(isUnread).length;
    return `<div class="fam${shut ? " shut" : ""}" data-fam="${esc(f)}" title="${
      kids.length} sessions - click to ${shut ? "show" : "hide"} them all, double-click to ${
      noWait ? "show" : "hide"} the ones waiting for you"><span class="car">${
      shut ? "▸" : "▾"}</span><span>${esc(f)}</span>${noWait && !shut ? `<span class="nowait">waiting hidden</span>` : ""
      }${nNew ? `<span class="newn" title="sessions that finished a turn since you last looked">${nNew} new</span>` : ""
      }<span class="dots">${dots}</span></div>` +
      (shown.length ? `<div class="kids">${shown.map(s => tile(s, true)).join("")}</div>` : "");
  }).join("");
}

// Folded project nodes, and nodes hiding their waiting sessions, per
// browser: a viewing convenience, not shared state.
const savedSet = name => new Set((() => {
  try { return JSON.parse(localStorage.getItem(name) || "[]"); }
  catch (e) { return []; }
})());
const folded = savedSet("ccontrol.folded"), waitHidden = savedSet("ccontrol.waithidden");
function flip(set, name, f){
  if (set.has(f)) set.delete(f); else set.add(f);
  try { localStorage.setItem(name, JSON.stringify([...set])); } catch (e) {}
  drawSessions();
}
// Unfolding always shows every session, the waiting ones included - the
// opposite of folding, which hides them all.
function toggleFold(f){
  if (folded.has(f) && waitHidden.has(f)){
    waitHidden.delete(f);
    try { localStorage.setItem("ccontrol.waithidden", JSON.stringify([...waitHidden])); } catch (e) {}
  }
  flip(folded, "ccontrol.folded", f);
}
const toggleWaiting = f => flip(waitHidden, "ccontrol.waithidden", f);

// A double click starts with a click, so a click waits briefly to see whether
// a second follows. Kept by node name, not element: the list is redrawn
// every 1.5 s and may replace the node between the two clicks.
const DOUBLE_MS = 300;
let famClick = null;
function famClicked(f){
  const prev = famClick;
  famClick = null;
  if (prev){
    clearTimeout(prev.t);
    if (prev.f === f) return toggleWaiting(f);
    toggleFold(prev.f);
  }
  famClick = {f, t: setTimeout(() => { famClick = null; toggleFold(f); }, DOUBLE_MS)};
}

function drawSessions(){
  // Whatever wants a human first, then this machine, then idle.
  sessions.sort((a,b) =>
    (a.attention?0:1) - (b.attention?0:1) ||
    (a.remote?1:0) - (b.remote?1:0) ||
    (stateOf(a)[0] === "d-idle" ? 1 : 0) - (stateOf(b)[0] === "d-idle" ? 1 : 0) ||
    nameOf(a).localeCompare(nameOf(b)));
  if (selected && !sessions.some(s => key(s) === selected)) selected = null;
  if (!selected && sessions.length) selected = key(sessions[0]);

  const q = ($("#filter").value || "").trim().toLowerCase();
  const hit = s => !q || [nameOf(s), s.name, s.project, s.cwd, s.window_title, s.doing, s.host]
    .some(v => String(v || "").toLowerCase().includes(q));
  const pinned = new Set(prefs.pinned || []), archived = new Set(prefs.archived || []);
  // Archive hides a session until it wants you; a blocked session always shows.
  const shown = sessions.filter(s => hit(s) && (!archived.has(pkey(s)) || s.attention));
  const hidden = sessions.filter(s => hit(s) && archived.has(pkey(s)) && !s.attention);

  const groups = new Map();
  const top = shown.filter(s => pinned.has(pkey(s)));
  if (top.length) groups.set("pinned", top);
  // A project's sessions stay together, in the group of its most urgent one
  // (the list is already sorted that way); each tile keeps its own dot.
  const famGroup = new Map();
  for (const s of shown){
    if (pinned.has(pkey(s))) continue;
    const f = projectOf(s);
    if (!famGroup.has(f)) famGroup.set(f, groupOf(s));
    const g = famGroup.get(f);
    if (!groups.has(g)) groups.set(g, []);
    groups.get(g).push(s);
  }
  let names = [...groups.keys()];
  if (prefs.group === "state")
    names.sort((a,b) => (a==="pinned"?-1:STATE_ORDER.indexOf(a)) - (b==="pinned"?-1:STATE_ORDER.indexOf(b)));
  else if (prefs.group === "folder")
    names.sort((a,b) => a==="pinned" ? -1 : b==="pinned" ? 1 : a.localeCompare(b));

  const byFolder = g => prefs.group === "folder" && g !== "pinned";
  let html = names.map(g => (!byFolder(g) && (g || names.length > 1) ?
      `<div class="g"><span>${esc(g || "sessions")}</span><span>${groups.get(g).length}</span></div>` : "") +
    tree(groups.get(g))).join("");
  if (hidden.length){
    html += `<div class="g fold" id="archfold"><span>${showArchived ? "▾" : "▸"} archived</span><span>${hidden.length}</span></div>`;
    if (showArchived) html += hidden.map(s => tile(s)).join("");
  }
  $("#sessions").innerHTML = html || `<div class="note">${q ? "no match" : "no sessions"}</div>`;

  document.querySelectorAll(".s").forEach(el =>
    el.onclick = () => { selected = el.dataset.k; showCached(); drawSessions(); refreshTab(true); loadStatus(true); });
  document.querySelectorAll("[data-fam]").forEach(el => el.onclick = () => famClicked(el.dataset.fam));
  document.querySelectorAll("[data-pin]").forEach(el => el.onclick = e => {
    e.stopPropagation(); savePrefs({pinned: toggleIn("pinned", el.dataset.pin)}); });
  document.querySelectorAll("[data-arch]").forEach(el => el.onclick = e => {
    e.stopPropagation(); savePrefs({archived: toggleIn("archived", el.dataset.arch)}); });
  document.querySelectorAll("[data-close]").forEach(el => el.onclick = e => {
    e.stopPropagation(); closeSession(el.dataset.close); });
  document.querySelectorAll(".badge.pr").forEach(el => el.onclick = e => {
    e.stopPropagation(); if (el.dataset.url) window.open(el.dataset.url, "_blank"); });
  const fold = $("#archfold");
  if (fold) fold.onclick = () => { showArchived = !showArchived; drawSessions(); };

  syncDraft();
  drawHead();
}

function drawHead(){
  const s = current();
  if (view !== "session") return;
  $("#title").textContent = s ? nameOf(s) : "no session selected";
  $("#sub").textContent = s ? [s.window_title, s.cwd].filter(Boolean).join("  ·  ") : "";
  $("#sub").title = $("#sub").textContent;
  $("#send").disabled = !s || !!s.remote;
  $("#sendmore").classList.toggle("off", !s || !!s.remote);
  $("#queue").disabled = !s;
  const acts = [];
  if (s && !s.remote){
    if (s.resumable){
      acts.push(`<button class="sm" data-act="resume-window" title="claude --resume in a new tab">Resume</button>`);
      acts.push(`<button class="sm ghost" data-act="resume-background" title="continue it in the background; the prompt box is its next message">Resume in bg</button>`);
    }
    if (s.pid)
      acts.push(`<button class="sm ghost" data-act="show" title="bring this session's terminal window to the front">Show Window</button>`);
  }
  const html = acts.join("");
  if ($("#acts").dataset.sig !== html){ $("#acts").innerHTML = html; $("#acts").dataset.sig = html; }
  // New, New+close, Fork and Fork in bg live in the New / Fork menu, and
  // Copy resume cmd in the View menu; neither applies to a remote session.
  const local = !!(s && !s.remote);
  $("#launch").hidden = !local;
  if (!local) $("#launch").open = false;
  $('#launch [data-act="new-close"]').hidden = !(local && s.pid);
  $("#copybtn").hidden = !local;
  $("#copybtn").title = local ? s.resume_command || "" : "";
  document.querySelectorAll("[data-act]").forEach(b => b.onclick = () => { closeMenus(); act(b.dataset.act); });
  drawQueued(s);
  drawHeavy(s);
}

function current(){ return sessions.find(x => key(x) === selected); }

function renderBlocks(blocks){
  return blocks.map(renderBlock).join("");
}

function renderBlock(b){
  if (b.kind === "text")     return renderText(b.text);
  if (b.kind === "thinking") return `<div class="think">${esc(b.text)}</div>`;
  if (b.kind === "tool")     return `<div class="tool"><b>${esc(b.name)}</b> ${esc(b.summary)}</div>`;
  if (b.kind === "result")   return `<div class="res${b.is_error?" err":""}">${
    esc(b.text)}${b.truncated?"\n…":""}</div>`;
  return "";
}

// -- how each kind of text is shown ----------------------------------------
// Full is the whole block, as it always was; Compact folds a block to its
// first line, opened by a click; Hidden leaves it out. Kept in this browser,
// the same for every session.
const SHAPES = [["prompt", "Prompts"], ["reply", "Replies"], ["thinking", "Thinking"],
  ["tool", "Tool calls"], ["result", "Tool results"], ["system", "System"]];
const MODES = ["full", "compact", "hidden"];
const SUMMARY_NOTE = "earlier conversation, summarised";
let shape = {};
try { shape = JSON.parse(localStorage.getItem("ccontrol.shape") || "{}") || {}; } catch (e) { shape = {}; }
function modeOf(cat){ return MODES.includes(shape[cat]) ? shape[cat] : "full"; }

function categoryOf(it, b){
  if (it.note) return "system";
  if (b.kind === "text") return it.speaker === "you" ? "prompt" : "reply";
  return b.kind;
}

function oneLine(b){
  const text = b.kind === "tool" ? b.summary : b.text;
  return String(text || "").split("\n").map(l => l.trim()).find(l => l) || "";
}

function compactBlock(b, k, open){
  const tag = b.kind === "tool" ? b.name : b.kind === "result" ? (b.is_error ? "error" : "result")
            : b.kind === "thinking" ? "thinking" : "";
  return `<details class="cmp" data-k="${k}"${open ? " open" : ""}><summary>${
    tag ? `<span class="k${b.is_error ? " err" : ""}">${esc(tag)}</span>` : ""}<span class="ln">${
    esc(oneLine(b))}</span></summary>${renderBlock(b)}</details>`;
}

// One turn's blocks as the controls say. `k` names a block by its place in
// the conversation, so one opened by hand stays open across repaints.
function shapeBlocks(it, i, opened){
  return it.blocks.map((b, j) => {
    const m = modeOf(categoryOf(it, b));
    if (m === "hidden") return "";
    const k = i + "." + j;
    return m === "compact" ? compactBlock(b, k, opened.has(k)) : renderBlock(b);
  }).join("");
}

function drawShape(){
  // The View menu's button is lit while any kind of text is not shown in full.
  $("#viewmenu>summary").classList.toggle("mod", SHAPES.some(([cat]) => modeOf(cat) !== "full"));
  $("#shape").innerHTML = SHAPES.map(([cat, label]) => {
    const m = modeOf(cat);
    return `<label>${label}<select class="tiny${m === "full" ? " full" : ""}" data-shape="${cat}">${
      MODES.map(x => `<option value="${x}"${x === m ? " selected" : ""}>${
        x[0].toUpperCase() + x.slice(1)}</option>`).join("")}</select></label>`;
  }).join("");
}

let logItems = [], opened = new Set(), openedFor = null;
// A long session is thousands of elements, and building them all was most of
// what switching to it cost, so the newest turns are drawn first and earlier
// ones on request. `logShown` is how many, per selected session.
const LOG_WINDOW = 60, LOG_STEP = 200;
let logShown = LOG_WINDOW;
// Pure: the index of the first item drawn.
const logStart = (n, shown) => Math.max(0, n - shown);

// Recently viewed sessions' items, so switching back paints at once and the
// poll that follows only has to confirm nothing changed.
const CONV_KEEP = 8;
const convCache = new Map();  // session key -> {items, sig}, oldest first
function rememberConv(k, items, sig){
  convCache.delete(k);
  convCache.set(k, {items, sig});
  while (convCache.size > CONV_KEEP) convCache.delete(convCache.keys().next().value);
}

function setLog(items, sig, force){
  lastSig = sig;
  logItems = items;
  // New turns can match an open search; earlier indexes stay where they were.
  if (finds.q || finds.filter){
    finds.hits = findHits(items, finds.q, finds.filter, blockShown);
    finds.at = Math.min(Math.max(finds.at, 0), finds.hits.length - 1);
  }
  drawLog(force);
  const asked = items.filter(it => it.speaker === "you").pop();
  currentPrompt = asked ? asked.blocks.filter(b => b.kind === "text").map(b => b.text).join("\n\n") : "";
  drawCurrent();
}

// A tile was clicked: show what this page last had for it, if anything.
function showCached(){
  resetFind();
  logFollow = true;
  const hit = convCache.get(selected);
  if (hit){ setLog(hit.items, hit.sig, true); return; }
  lastSig = "";
  logItems = [];
  $("#log").innerHTML = `<div class="note">loading…</div>`;
}

function showEarlier(all){
  const log = $("#log"), below = log.scrollHeight - log.scrollTop;
  logShown = all ? logItems.length : logShown + LOG_STEP;
  drawLog();
  log.scrollTop = log.scrollHeight - below;  // keep what was in view in view
}

function drawLog(force){
  if (!selected) return;
  const log = $("#log");
  if (openedFor !== selected){ opened = new Set(); openedFor = selected; logShown = LOG_WINDOW; }
  if (force) logFollow = true;
  const start = logStart(logItems.length, logShown);
  const earlier = start ? `<div class="earlier"><button data-earlier="1">show ${Math.min(start, LOG_STEP)} earlier</button>${
    start > LOG_STEP ? ` <button data-earlier="all">show all ${start}</button>` : ""}</div>` : "";
  const findSet = new Set(finds.hits);
  log.innerHTML = earlier + logItems.slice(start).map((it, j) => {
    const i = start + j;
    // Where the session compacted: the summary that replaced everything above
    // it, folded away, since the real conversation is now shown in full.
    if (it.note === SUMMARY_NOTE)
      return modeOf("system") === "hidden" ? "" : `<details class="compact"><summary>compacted here - show the summary it kept</summary>${
        renderBlocks(it.blocks)}</details>`;
    const body = shapeBlocks(it, i, opened);
    if (!body) return "";
    return `<div class="turn${it.speaker === "you" ? " you-turn" : ""}${findSet.has(i) ? " found" : ""}" data-i="${i}"><div class="who ${esc(it.speaker || it.role)}">${
      esc(it.note || SPEAKER[it.speaker] || it.speaker || it.role)}${it.via ? ` <span class="via" title="arrived through the session inbox: the window, the palette, impctl or another session">via inbox</span>` : ""}</div>${body}</div>`;
  }).join("") || `<div class="note">${logItems.length ? "everything here is hidden - see the display settings below" : "nothing yet"}</div>`;
  log.insertAdjacentHTML("beforeend", `<div class="newest"><div class="float"><div class="cold" hidden></div>` +
    `<button data-newest="1" title="go to the end and follow it again">Show Most Recent</button></div></div>`);
  drawCold(current());
  markFinds();
  if (logFollow) log.scrollTop = log.scrollHeight;
  markAway();
}

// -- following the end of the conversation ---------------------------------
// The log follows new text while its end is in view. Only the reader's own
// scrolling changes that: scrolling up stops it, scrolling back to the end
// (or Show Most Recent) starts it again. Whether to follow is not re-measured
// on each redraw, because the log also shrinks under the reader - the process
// drawer opening, the status panel unfolding, the prompt box growing - and
// that pushed the end out of view and quietly stopped it following.
let logFollow = true;
const LOG_END = 24;  // px from the end that still counts as at the end
const logAtEnd = log => log.scrollHeight - log.scrollTop - log.clientHeight <= LOG_END;
function markAway(){
  const log = $("#log");
  log.classList.toggle("away", !logAtEnd(log));
}
function showNewest(){
  const log = $("#log");
  logFollow = true;
  log.scrollTop = log.scrollHeight;
  markAway();
}

async function loadLog(force){
  if (!selected) { $("#log").innerHTML = `<div class="note">select a session</div>`; return; }
  let data;
  // The whole conversation, as the terminal shows it; the dispatcher answers
  // "unchanged" when what this page already has is current - including
  // what it had cached from an earlier visit.
  const want = selected;
  const known = lastSig ? "&known=" + encodeURIComponent(lastSig) : "";
  try { data = await get("/api/conversation?session=" + encodeURIComponent(want) + known); }
  catch (e) { return; }
  // A reply for a session no longer selected (a poll still in flight when a
  // tile was clicked) must not paint over the new one.
  if (want !== selected) {
    if (data.items && data.sig) rememberConv(want, data.items, data.sig);
    return;
  }
  if (data.unchanged) return;
  if (data.error && !data.items.length){
    $("#log").innerHTML = `<div class="note">${esc(data.error)}</div>`;
    return;
  }
  // Only repaint when something changed, so the scroll position survives.
  const sig = data.sig || (JSON.stringify(data.items).length + ":" + data.items.length);
  rememberConv(want, data.items, sig);
  if (!force && sig === lastSig) return;
  setLog(data.items, sig, force);
}

// -- the prompt being worked on --------------------------------------------
// The last thing you asked, pinned above the chat on demand, so a long turn
// can be read against what it is meant to be doing.
let currentPrompt = "", showCurrent = false, currentFolded = false;
function drawCurrent(){
  const box = $("#current");
  box.hidden = !showCurrent || tab !== "log" || view !== "session";
  $("#curbtn").textContent = showCurrent ? "Hide Current Prompt" : "Show Current Prompt";
  box.classList.toggle("fold", currentFolded);
  $("#curfold").innerHTML = currentFolded ? "&#9656;" : "&#9662;";
  $("#curbody").innerHTML = currentPrompt ? renderText(currentPrompt)
    : `<div class="note" style="padding:6px 0">no prompt yet in this session</div>`;
}

// -- the selected session, in numbers --------------------------------------
// Under the send buttons: what the tile says, plus what only the transcript
// and process tree know - background work, the prompt cache, tokens, cost.
let statusData = null, statusFor = null, statusAt = 0, statusSkew = 0, statusFolded = false;
try { statusFolded = localStorage.getItem("ccontrol.statusFolded") === "1"; } catch (e) {}

function kfmt(n){
  if (n == null) return "";
  if (n >= 1e9) return (n/1e9).toFixed(2) + "B";
  if (n >= 1e6) return (n/1e6).toFixed(n >= 1e7 ? 0 : 1) + "M";
  if (n >= 1e3) return (n/1e3).toFixed(n >= 1e4 ? 0 : 1) + "k";
  return String(Math.round(n));
}

function span(sec){
  sec = Math.max(0, Math.round(sec));
  if (sec < 60) return sec + "s";
  if (sec < 3600) return Math.round(sec/60) + "m";
  const h = Math.floor(sec/3600), m = Math.round((sec % 3600)/60);
  if (h < 24) return h + "h" + (m ? " " + m + "m" : "");
  return Math.floor(h/24) + "d " + (h % 24) + "h";
}

const plural = (n, word) => n + " " + word + (n === 1 ? "" : "s");

async function loadStatus(force){
  if (!selected){ $("#status").hidden = true; return; }
  if (force || statusFor !== selected || Date.now() - statusAt >= 3000){
    statusAt = Date.now();
    const want = selected;
    try {
      const d = await get("/api/session_status?session=" + encodeURIComponent(want));
      if (want !== selected) return;
      // The open tile says what the session has cost: redrawn when that moves.
      const cost = s => s && s.spend ? s.spend.cost_usd : null;
      const moved = cost(d) !== cost(statusFor === want ? statusData : null);
      statusData = d; statusFor = want;
      if (d.now) statusSkew = d.now - Date.now()/1000;
      if (moved) drawSessions();
    } catch (e) {}
  }
  drawStatus();
}

// Pure: the panel's HTML for row `s` and its /api/session_status answer `d`.
function statusHtml(s, d, now, folded){
  const st = d.stat || {}, u = s.usage || {}, have = !!d.stat;
  const [dot, label] = stateOf(s);
  const cells = [], sum = [`<span class="dot ${dot}"></span>${esc(label)}`];
  const cell = (k, v, title, cls) => {
    if (v === "" || v == null) return;
    cells.push(`<div class="c" title="${esc(title || "")}"><b>${esc(k)}</b><span class="${
      cls || ""}">${esc(v)}</span></div>`);
  };
  const since = s.attention_since || s.status_changed_at;
  cell("state", label + (since ? " · " + age(since) : ""), s.attention_reason || s.doing || "");
  const ag = st.agents || [], sh = st.shells || [], mo = st.monitors || [];
  if (have){
    cell("subagents", String(ag.length), ag.map(a => a.label).join("\n"), ag.length ? "hot" : "");
    cell("shells", String(sh.length), "commands it put in the background and has not seen finish\n" +
      sh.map(x => x.label).join("\n"), sh.length ? "hot" : "");
    cell("monitors", String(mo.length), mo.map(m => m.label).join("\n"), mo.length ? "hot" : "");
    if (d.shell_processes != null)
      cell("processes", String(d.shell_processes), "shell processes running under its claude " +
        "process - commands and monitors alike, read from the process tree",
        d.shell_processes ? "hot" : "");
  }
  if (ag.length) sum.push(plural(ag.length, "subagent"));
  if (sh.length) sum.push(plural(sh.length, "shell"));
  if (mo.length) sum.push(plural(mo.length, "monitor"));
  const c = st.cache;
  if (c && c.expires_at){
    const left = c.expires_at - now;
    cell("cache", (left > 0 ? `warm · ${span(left)} left` : `cold · ${span(-left)} ago`) +
      (c.hit != null ? ` · ${Math.round(c.hit * 100)}% hit` : ""),
      `prompt cache: ${c.ttl >= 3600 ? "1 hour" : "5 minute"} lifetime from the last reply; ` +
      "a prompt after it expires re-sends the whole context at full price. " +
      "Hit is the share of the last request read from cache.", left > 0 ? "ok" : "warn");
    // Negative, like the tile's: it is counting down to expiry.
    sum.push(left > 0 ? `cache -${span(left)}` : "cache cold");
  }
  if (u.context_pct != null || st.context_tokens)
    cell("context", [u.context_pct != null ? Math.round(u.context_pct) + "%" : "",
      st.context_tokens ? kfmt(st.context_tokens) + (u.context_size ? " / " + kfmt(u.context_size) : "") : ""
    ].filter(Boolean).join(" · "), "", u.context_pct >= 80 ? "warn" : "");
  const sp = d.spend;
  if (sp && sp.cost_usd != null){
    cell("cost", "≈$" + sp.cost_usd.toFixed(2), "the whole session, subagents included, estimated at API list prices" +
      (u.cost_usd != null ? `; Claude Code reports $${u.cost_usd.toFixed(2)} for its current run` : ""));
    sum.push("≈$" + sp.cost_usd.toFixed(2));
  } else if (u.cost_usd != null){
    cell("cost", "$" + u.cost_usd.toFixed(2), "as Claude Code reports it to the statusline, for its current run");
    sum.push("$" + u.cost_usd.toFixed(2));
  }
  const t = (sp && sp.tokens) || st.tokens;
  if (t){
    cell("tokens", `${kfmt(t.input + t.cache_read + t.cache_write)} in · ${kfmt(t.output)} out`,
      `input ${t.input}\ncache read ${t.cache_read}\ncache write ${t.cache_write}\noutput ${t.output}` +
      (sp && sp.tokens ? "\n(the whole session, subagents included)" : "\n(this session's transcript; subagents keep their own)"));
    cell("cached", `${kfmt(t.cache_read)} read · ${kfmt(t.cache_write)} written`);
  }
  cell("model", u.model || st.model_id || "");
  cell("mode", st.permission_mode || "", "permission mode");
  if (st.prompts != null) cell("prompts", st.prompts + (st.turns ? ` · ${st.turns} turns` : ""));
  if (st.last_turn_ms) cell("last turn", span(st.last_turn_ms / 1000));
  if (st.started) cell("age", span(now - st.started));
  if (u.api_duration_ms)
    cell("API time", span(u.api_duration_ms / 1000) + (u.duration_ms ? " of " + span(u.duration_ms / 1000) : ""));
  if (u.lines_added != null || u.lines_removed != null)
    cell("lines", `+${u.lines_added || 0} −${u.lines_removed || 0}`);
  if (st.compactions) cell("compactions", String(st.compactions));
  if (s.queued) cell("queued", String(s.queued));
  cell("branch", st.branch || "");
  if (st.version) cell("version", st.version, "Claude Code");
  if (d.note) cell("note", d.note);
  if (d.error) cell("error", d.error, "", "warn");
  const run = [...ag.map(x => ["agent", x]), ...sh.map(x => ["shell", x]), ...mo.map(x => ["monitor", x])]
    .map(([k, x]) => `<div class="ri"><span class="k">${k}${x.foreground ? " · now" : ""}</span>${
      esc(x.label || x.id)}${x.started ? " · " + age(x.started) : ""}${
      x.expires ? " · ends in " + span(x.expires - now) : ""}</div>`).join("");
  return `<div class="hd"><span class="sum">${sum.join(" · ")}</span><span>${folded ? "&#9656;" : "&#9662;"}</span></div>` +
    `<div class="bd"><div class="grid">${cells.join("")}</div>${run ? `<div class="run">${run}</div>` : ""}</div>`;
}

function drawStatus(){
  const box = $("#status"), s = current();
  box.hidden = !s;
  if (!s) return;
  const d = statusFor === selected && statusData ? statusData : {};
  box.classList.toggle("fold", statusFolded);
  box.innerHTML = statusHtml(s, d, Date.now() / 1000 + statusSkew, statusFolded);
}

// -- what the session is waiting on ------------------------------------------
// A tab per shell, monitor or subagent the selected session is waiting on,
// showing the end of what it has printed - for a subagent, what it has said
// and the tools it called (read from the files Claude Code writes them to).
// Several get an Auto tab first, which follows whichever printed last.
let procTasks = [], procFor = null, procPick = "auto", procShown = "";
const PROC_MARK = {monitor: "mon", agent: "agent"};

// Pure: the tabs and the task to show for `tasks` with `pick` chosen.
function procView(tasks, pick){
  if (!tasks.length) return {tabs: [], show: null, pick: "auto"};
  const latest = tasks.reduce((a, b) =>
    ((b.mtime || 0) > (a.mtime || 0) || ((b.mtime || 0) === (a.mtime || 0)
      && (b.started || 0) >= (a.started || 0))) ? b : a);
  const auto = tasks.length > 1;
  if (pick !== "auto" && !tasks.some(t => t.id === pick)) pick = "auto";
  if (!auto && pick === "auto") pick = tasks[0].id;
  const show = pick === "auto" ? latest : tasks.find(t => t.id === pick);
  const tabs = tasks.map(t => ({id: t.id, kind: t.kind, label: t.label || t.id,
                                on: t.id === pick, live: pick === "auto" && t === latest}));
  if (auto) tabs.unshift({id: "auto", label: "Auto", on: pick === "auto"});
  return {tabs, show, pick};
}

function procWanted(){
  const s = current(), w = (s && s.work) || {};
  return view === "session" && tab === "log" && s && !s.remote && w.alive
    && ((w.shells || 0) + (w.monitors || 0) + (w.agents || 0) > 0);
}

async function loadProcs(){
  const want = selected;
  if (!procWanted()){ procTasks = []; procFor = want; drawProcs(); return; }
  let d;
  try { d = await get("/api/awaited?session=" + encodeURIComponent(want)); } catch (e) { return; }
  if (want !== selected || !d.ok) return;
  if (procFor !== want) procPick = "auto";
  procTasks = d.tasks || []; procFor = want;
  drawProcs();
}

function drawProcs(){
  const box = $("#procs"), v = procView(procFor === selected ? procTasks : [], procPick);
  procPick = v.pick;
  box.hidden = !v.show;
  if (!v.show){ procShown = ""; return; }
  const tabs = v.tabs.map(t => `<button role=tab data-proc="${esc(t.id)}" class="${t.on ? "on" : ""}${
      t.live ? " live" : ""}" title="${esc(t.id === "auto" ? "follows whichever printed last" : t.label)}">${
      t.kind ? `<span class=k>${PROC_MARK[t.kind] || "$"}</span>` : ""}${esc(t.label)}</button>`).join("");
  if ($("#proctabs").innerHTML !== tabs) $("#proctabs").innerHTML = tabs;
  const pre = $("#proctext"), text = v.show.text || "";
  const sig = v.show.id + "\n" + text;
  if (sig === procShown) return;
  // Keep following the end unless the reader has scrolled up to look.
  const atEnd = pre.scrollHeight - pre.scrollTop - pre.clientHeight < 4
    || !procShown.startsWith(v.show.id + "\n");
  pre.innerHTML = text.trim() ? esc(text.replace(/\n+$/, "")) : `<span class=none>no output yet</span>`;
  procShown = sig;
  if (atEnd) pre.scrollTop = pre.scrollHeight;
}

async function procLoop(){
  try { await loadProcs(); } catch (e) {}
  setTimeout(procLoop, 1000);
}

$("#proctabs").addEventListener("click", e => {
  const b = e.target.closest("[data-proc]");
  if (!b) return;
  procPick = b.dataset.proc; procShown = ""; drawProcs();
});
try { const h = +localStorage.getItem("ccontrol.procsHeight");
      if (h > 0) $("#procs").style.setProperty("--ph", h + "px"); } catch (e) {}
$("#procgrip").addEventListener("pointerdown", e => {
  const box = $("#procs"), y0 = e.clientY, h0 = box.offsetHeight;
  const most = $("#sessionpane").clientHeight * 0.8;
  e.preventDefault(); box.classList.add("drag");
  e.target.setPointerCapture(e.pointerId);
  const move = m => box.style.setProperty("--ph",
    Math.round(Math.max(64, Math.min(most, h0 + y0 - m.clientY))) + "px");
  const up = () => {
    box.classList.remove("drag");
    e.target.removeEventListener("pointermove", move);
    e.target.removeEventListener("pointerup", up);
    try { localStorage.setItem("ccontrol.procsHeight", String(box.offsetHeight)); } catch (x) {}
  };
  e.target.addEventListener("pointermove", move);
  e.target.addEventListener("pointerup", up);
});

// -- jump between your prompts ---------------------------------------------
function jump(dir){
  const log = $("#log");
  const top = log.scrollTop, turns = [...log.querySelectorAll(".turn.you-turn")];
  // Compare where each prompt would be scrolled to, not where it sits, or the
  // prompt just jumped to counts as the next one down.
  const at = x => x.offsetTop - 8;
  const t = dir < 0 ? turns.filter(x => at(x) < top - 2).pop()
                    : turns.find(x => at(x) > top + 2);
  // Above the first drawn prompt: draw the earlier turns and go on from there.
  if (!t && dir < 0 && logStart(logItems.length, logShown) > 0){ showEarlier(); return jump(dir); }
  if (!t) return;
  log.scrollTop = at(t);
  t.classList.add("flash");
  setTimeout(() => t.classList.remove("flash"), 900);
}

// -- one unsent draft per session ------------------------------------------
// The box belongs to whichever session it was typed for; switching sessions
// parks it and brings back that session's own. Kept in this browser only.
let drafts = {}, draftFor = null;
try { drafts = JSON.parse(localStorage.getItem("ccontrol.drafts") || "{}") || {}; } catch (e) { drafts = {}; }
function saveDrafts(){
  try { localStorage.setItem("ccontrol.drafts", JSON.stringify(drafts)); } catch (e) {}
}
function noteDraft(){
  if (draftFor === null) return;
  const v = $("#prompt").value;
  if (v) drafts[draftFor] = v; else delete drafts[draftFor];
  saveDrafts();
}
function syncDraft(){
  if (draftFor === selected) return;
  noteDraft();
  draftFor = selected;
  $("#prompt").value = (selected && drafts[selected]) || "";
}
// After a send: clear the draft it came from, and the box only if it still
// shows that session - the person may have moved on while it was in flight.
function clearPrompt(k){
  k = k === undefined ? draftFor : k;
  delete drafts[k]; saveDrafts();
  if (draftFor === k) $("#prompt").value = "";
}

// -- what changed since you last looked --------------------------------------
// A session that finished a turn while you were looking elsewhere is marked
// "new" until you select it. When it went idle is its status change time.
let seenAt = {};
try { seenAt = JSON.parse(localStorage.getItem("ccontrol.seen") || "{}") || {}; } catch (e) { seenAt = {}; }
const doneAt = s => s.status === "idle" && s.status_changed_at ? s.status_changed_at : 0;
function isUnread(s){ return key(s) !== selected && doneAt(s) > (seenAt[key(s)] || 0) + 1; }

// Pure: the seen map after a new session list. The session being looked at
// is seen now; one met for the first time counts as seen as it stands, so
// opening the window does not mark everything new; gone ones drop out.
function updateSeen(seen, list, looking, now){
  if (!list.length) return seen;
  const out = {};
  for (const s of list){
    const k = key(s);
    out[k] = k === looking ? Math.max(now, doneAt(s)) : (k in seen ? seen[k] : doneAt(s));
  }
  return out;
}

// -- attention: title, favicon, desktop notifications -------------------------
let attnBefore = null, deskNotify = false;
try { deskNotify = localStorage.getItem("ccontrol.desknotify") === "1"; } catch (e) {}

// Pure: sessions that started waiting for you since the last list (none on
// the first, so opening the window does not announce what already waits).
function newlyWaiting(before, list){
  if (!before) return [];
  return list.filter(s => s.attention && !before.has(key(s)));
}

// Pure: the session to switch to because it just started - one that was not
// on the last list and started within two minutes, the newest if several.
// None on the first list, and none without a start time, so a remote host
// reconnecting or the dispatcher restarting does not yank the view.
function newlyStarted(before, list, now){
  if (!before) return null;
  const at = s => { const t = Number(s.startedAt) || 0; return t > 1e12 ? t / 1000 : t; };
  const fresh = list.filter(s => !before.has(key(s)) && at(s) && now - at(s) < 120)
                    .sort((a, b) => at(b) - at(a));
  return fresh.length ? key(fresh[0]) : null;
}

// Pure: the title counts the sessions waiting for you. hotkey.py accepts the
// "(N) " prefix when it looks for this window by title.
const titleFor = n => (n ? `(${n}) ` : "") + "Imperatorium";

function notifyWaiting(list){
  const fresh = newlyWaiting(attnBefore, list);
  attnBefore = new Set(list.filter(s => s.attention).map(key));
  if (!fresh.length || !deskNotify || !("Notification" in window) || Notification.permission !== "granted") return;
  if (document.visibilityState === "visible" && document.hasFocus()) return;
  for (const s of fresh.slice(0, 3)){
    const n = new Notification(nameOf(s) + " needs you", {body: stateOf(s)[1], tag: "imperatorium-" + key(s)});
    n.onclick = () => { window.focus(); select(key(s)); n.close(); };
  }
}

// The tab's icon is the most urgent state's dot, in its colour and shape.
let favSig = "";
function drawFavicon(list){
  const has = f => list.some(f);
  const state = has(s => s.attention && s.attention_kind !== "failure") ? "attn"
    : has(s => s.attention) ? "fail" : has(s => stateOf(s)[0] === "d-busy") ? "busy"
    : list.length ? "idle" : "none";
  const sig = state + matchMedia("(prefers-color-scheme: dark)").matches;
  if (sig === favSig) return;
  favSig = sig;
  const c = getComputedStyle(document.documentElement).getPropertyValue(
    {attn: "--warn", fail: "--fail", busy: "--busy", idle: "--ok", none: "--done"}[state]).trim() || "#888";
  const shape = state === "attn" ? `<rect x="7" y="7" width="18" height="18" rx="3" transform="rotate(45 16 16)" fill="${c}"/>`
    : state === "fail" ? `<path d="M8 8L24 24M24 8L8 24" stroke="${c}" stroke-width="6" stroke-linecap="round"/>`
    : state === "idle" ? `<circle cx="16" cy="16" r="10" fill="none" stroke="${c}" stroke-width="5"/>`
    : `<circle cx="16" cy="16" r="12" fill="${c}"/>`;
  $("#fav").href = "data:image/svg+xml," + encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">${shape}</svg>`);
}

// -- a new session list, pushed or polled -------------------------------------
const rowSig = s => [s.status, s.status_changed_at, s.last_event_at, s.doing, s.attention].join("|");
let keysBefore = null;
function gotSessions(list){
  const was = current(), wasSig = was ? rowSig(was) : "";
  sessions = list;
  catchFollowed();
  catchStarted(newlyStarted(keysBefore, list, Date.now() / 1000));
  keysBefore = new Set(list.map(key));
  const looking = view === "session" && document.visibilityState === "visible" ? selected : null;
  seenAt = updateSeen(seenAt, list, looking, Date.now() / 1000);
  try { localStorage.setItem("ccontrol.seen", JSON.stringify(seenAt)); } catch (e) {}
  notifyWaiting(list);
  document.title = titleFor(list.filter(s => s.attention).length);
  drawFavicon(list);
  drawSessions();
  // News about the open session: read its conversation now, not next tick.
  const now = current();
  if (now && was && key(now) === key(was) && rowSig(now) !== wasSig) refreshTab(false);
}

// Server-sent events: the dispatcher pushes the list when it changes, and at
// once when a hook reports. Polling takes over while the stream is down;
// EventSource reconnects by itself.
let streamUp = false;
function openStream(){
  if (!("EventSource" in window)) return;
  const es = new EventSource("/api/stream");
  es.addEventListener("sessions", e => {
    let d;
    try { d = JSON.parse(e.data); } catch (x) { return; }
    streamUp = true;
    gotSessions(d.sessions || []);
  });
  es.onerror = () => { streamUp = false; };
}

// -- keyboard ----------------------------------------------------------------
// j/k move through the list as shown, n goes to the next session waiting for
// you, / filters; alt+1-9, alt+N and ctrl+K work from inside the prompt box too.
const typing = e => { const t = e.target;
  return !!t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName)); };
const visibleKeys = () => [...document.querySelectorAll("#sessions .s")].map(el => el.dataset.k);

// Pure: the next session waiting for you after `current`, the longest
// waiting first, wrapping round.
function nextWaiting(list, current){
  const w = list.filter(s => s.attention).sort((a, b) =>
    (a.attention_since || 0) - (b.attention_since || 0) || nameOf(a).localeCompare(nameOf(b)));
  if (!w.length) return null;
  const i = w.findIndex(s => key(s) === current);
  return key(w[(i + 1) % w.length]);
}

function pick(k){
  if (!k) return;
  select(k);
  const el = [...document.querySelectorAll("#sessions .s")].find(x => x.dataset.k === k);
  if (el) el.scrollIntoView({block: "nearest"});
}
function stepSession(dir){
  const ks = visibleKeys();
  if (!ks.length) return;
  const i = ks.indexOf(selected);
  pick(ks[i < 0 ? 0 : (i + dir + ks.length) % ks.length]);
}
function goWaiting(){
  const k = nextWaiting(sessions, selected);
  if (k) pick(k); else $("#say").textContent = "nothing is waiting for you";
}

// ctrl+K: jump to any session by name.
let jsel = 0;
function jumpHits(){
  const q = $("#jq").value.trim().toLowerCase();
  return sessions.filter(s => !q || [nameOf(s), s.project, s.cwd, s.window_title, s.doing, s.host]
    .some(v => String(v || "").toLowerCase().includes(q))).slice(0, 50);
}
function drawJump(){
  const hit = jumpHits();
  jsel = Math.max(0, Math.min(jsel, hit.length - 1));
  $("#jlist").innerHTML = hit.map((s, i) => {
    const [dot, label] = stateOf(s);
    return `<div class="ji${i === jsel ? " on" : ""}" data-jk="${esc(key(s))}"><span class="dot ${dot}"></span><span class="jn">${
      esc(nameOf(s))}</span><span class="jw">${esc(label)}${s.window_title ? " · " + esc(s.window_title) : ""}</span></div>`;
  }).join("") || `<div class="note">no match</div>`;
  const on = $("#jlist .ji.on");
  if (on) on.scrollIntoView({block: "nearest"});
  return hit;
}
function openJump(show){
  $("#jumpbox").hidden = !show;
  if (show){ $("#jq").value = ""; jsel = 0; drawJump(); $("#jq").focus(); }
}

// -- heavy sessions ------------------------------------------------------------
// A full context, or a large one behind an expired prompt cache, makes the
// next prompt expensive: said on the tile and above the prompt box.
const HEAVY_TOKENS = 100000, CTX_WARN = 85;
function heavyWarnings(s, now){
  const out = [], w = s.work || {}, ctx = (s.usage || {}).context_pct;
  if (ctx != null && ctx >= CTX_WARN)
    out.push({kind: "context", short: "ctx " + Math.round(ctx) + "%",
      text: `context ${Math.round(ctx)}% full - consider /compact or a fresh session before more work`});
  const tok = w.context_tokens;
  if (tok >= HEAVY_TOKENS && w.cache_expires_at && w.cache_expires_at <= now)
    out.push({kind: "cold", short: "cold " + kfmt(tok),
      text: `prompt cache expired: the next prompt re-sends ${kfmt(tok)} tokens of context at the full cache-write price`});
  return out;
}
// A cold cache is not said above the prompt box but in the chat, as a button
// over its end that offers the way out: a fresh session, or one with this
// session closed.
function drawHeavy(s){
  const html = (s ? heavyWarnings(s, Date.now() / 1000) : []).filter(h => h.kind !== "cold")
    .map(h => `<div>${esc(h.text)}</div>`).join("");
  if ($("#heavy").dataset.sig !== html){ $("#heavy").innerHTML = html; $("#heavy").dataset.sig = html; }
  drawCold(s);
}

// Pure: the cold-cache button for row `s`, or "" while its cache is warm, it
// is small, or it is not on this machine.
function coldHtml(s, now){
  if (!s || s.remote || !heavyWarnings(s, now).some(h => h.kind === "cold")) return "";
  const w = s.work, cost = w.resume_cost_usd;
  return `<div class="coldbtn" role="group" aria-label="prompt cache expired"><div class="ct"><b>Prompt cache expired.</b> ` +
    `Continuing this session re-sends ${esc(kfmt(w.context_tokens))} tokens of context` +
    (cost != null ? ` - about ${esc(usd(cost))} at the cache-write price` : " at the full cache-write price") +
    `. Click to start a new session, or to close this one and start a new session.` +
    `<div class="cn">Or just carry on: a prompt entered as usual continues this session.</div></div><div class="segs">` +
    `<button data-coldact="new" title="a fresh session in this project, selected here; this one stays open">New session</button>` +
    (s.pid ? `<button data-coldact="new-close" title="a fresh session in this project, selected here; this one is then closed (its conversation stays resumable)">Close this &amp; start new</button>` : "") +
    `</div></div>`;
}
function drawCold(s){
  const box = $("#log .newest .cold");
  if (!box) return;
  const html = coldHtml(s, Date.now() / 1000);
  if (box.dataset.sig !== html){ box.innerHTML = html; box.dataset.sig = html; box.hidden = !html; }
  const log = $("#log"), was = log.classList.contains("cold");
  log.classList.toggle("cold", !!html);
  if (!!html !== was && logFollow) log.scrollTop = log.scrollHeight;
}

// -- find in the conversation ----------------------------------------------------
let finds = {q: "", filter: "", hits: [], at: -1};
const blockText = b => b.kind === "tool" ? (b.name || "") + " " + (b.summary || "") : String(b.text || "");

// Pure: the indexes of the items that match. "tool" looks only at tool calls
// and "fail" only at failed results (all of them when `q` is empty); with no
// filter every block `shown` lets through is searched, and "" finds nothing.
function findHits(items, q, filter, shown){
  q = String(q || "").toLowerCase();
  if (!q && !filter) return [];
  const out = [];
  items.forEach((it, i) => {
    if ((it.blocks || []).some(b => {
      if (shown && !shown(it, b)) return false;
      if (filter === "tool" && b.kind !== "tool") return false;
      if (filter === "fail" && !(b.kind === "result" && b.is_error)) return false;
      return !q || blockText(b).toLowerCase().includes(q);
    })) out.push(i);
  });
  return out;
}
const blockShown = (it, b) => modeOf(it.note === SUMMARY_NOTE ? "system" : categoryOf(it, b)) !== "hidden";

function runFind(){
  finds.q = $("#cfind").value.trim();
  finds.filter = $("#cfilter").value;
  finds.hits = findHits(logItems, finds.q, finds.filter, blockShown);
  finds.at = finds.hits.length;   // the first step lands on the newest
  drawLog();
  goFind(-1);
}
function resetFind(){
  finds = {q: "", filter: "", hits: [], at: -1};
  $("#cfind").value = ""; $("#cfilter").value = ""; $("#cfcount").textContent = "";
}
function goFind(dir){
  const n = finds.hits.length;
  if (!n){ $("#cfcount").textContent = finds.q || finds.filter ? "0" : ""; return; }
  finds.at = ((finds.at + dir) % n + n) % n;
  const i = finds.hits[finds.at];
  if (i < logStart(logItems.length, logShown)){ logShown = logItems.length - i + 2; drawLog(); }
  $("#cfcount").textContent = `${finds.at + 1}/${n}`;
  markFinds();
  const el = [...document.querySelectorAll("#log .turn[data-i]")].find(t => +t.dataset.i === i);
  if (!el) return;
  const q = finds.q.toLowerCase();
  el.querySelectorAll("details.cmp:not([open])").forEach(d => {
    if (!q || d.textContent.toLowerCase().includes(q)){ d.open = true; opened.add(d.dataset.k); }
  });
  const m = el.querySelector("mark.cur") || el;
  $("#log").scrollTop = Math.max(0, m.offsetTop - $("#log").clientHeight / 3);
}
// Highlights every match in the drawn turns, and the current one strongest.
function markFinds(){
  const q = finds.q.toLowerCase(), cur = finds.hits[finds.at];
  document.querySelectorAll("#log .turn.found").forEach(t => {
    if (q && !t.dataset.marked){ wrapMatches(t, q); t.dataset.marked = "1"; }
    t.classList.toggle("cur-find", +t.dataset.i === cur);
  });
  document.querySelectorAll("#log mark.cur").forEach(m => m.classList.remove("cur"));
  const ct = $("#log .turn.cur-find mark");
  if (ct) ct.classList.add("cur");
}
function wrapMatches(root, q){
  const walk = document.createTreeWalker(root, NodeFilter.SHOW_TEXT), nodes = [];
  while (walk.nextNode()) nodes.push(walk.currentNode);
  for (const n of nodes){
    const text = n.nodeValue, low = text.toLowerCase();
    if (!low.includes(q)) continue;
    const frag = document.createDocumentFragment();
    let at = 0, i;
    while ((i = low.indexOf(q, at)) >= 0){
      frag.append(text.slice(at, i));
      const m = document.createElement("mark");
      m.textContent = text.slice(i, i + q.length);
      frag.append(m);
      at = i + q.length;
    }
    frag.append(text.slice(at));
    n.replaceWith(frag);
  }
}

// -- diff rows, for line comments -------------------------------------------------
// Pure: each line of a unified diff with its class and, for lines inside a
// hunk, the file and line it is on - the new side for added and context
// lines, the old side for removed ones.
function diffRows(text){
  let file = null, oldN = 0, newN = 0, inHunk = false;
  return String(text || "").split("\n").map(l => {
    if (l.startsWith("diff --git")){
      inHunk = false;
      const m = / b\/(.*)$/.exec(l);
      file = m ? m[1] : null;
      return {cls: "f", text: l};
    }
    if (!inHunk && l.startsWith("+++ ")){ const p = l.slice(4); if (p !== "/dev/null") file = p.replace(/^b\//, ""); return {cls: "", text: l}; }
    const h = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(l);
    if (h){ oldN = +h[1]; newN = +h[2]; inHunk = true; return {cls: "hh", text: l}; }
    if (!inHunk || !file || l.startsWith("\\")) return {cls: "", text: l};
    if (l.startsWith("+")) return {cls: "a", text: l, file, line: newN++, side: "new"};
    if (l.startsWith("-")) return {cls: "r", text: l, file, line: oldN++, side: "old"};
    if (l.startsWith(" ")){ oldN++; return {cls: "", text: l, file, line: newN++, side: "new"}; }
    inHunk = false;
    return {cls: "", text: l};
  });
}

// Pure: the comments as one prompt for the session.
function reviewPrompt(list){
  return "Review comments on your uncommitted changes, left on the diff in Imperatorium's " +
    "Changes tab. Please address each one:\n\n" + list.map(c => {
      const where = c.side === "old" ? `${c.file} (removed line ${c.line})` : `${c.file}:${c.line}`;
      const code = String(c.code || "").trim();
      return `- \`${where}\`${code ? " - `" + code.slice(0, 160).replace(/`/g, "'") + "`" : ""}\n  ${
        c.text.replace(/\n/g, "\n  ")}`;
    }).join("\n");
}

// -- cost history ------------------------------------------------------------------
const niceMax = v => { const p = Math.pow(10, Math.floor(Math.log10(v)));
  return [1, 2, 2.5, 5, 10].map(m => m * p).find(m => m >= v); };
const dayLabel = s => { const [y, m, d] = s.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString([], {month: "short", day: "numeric"}); };
const dollars = v => "$" + (v >= 100 ? v.toFixed(0) : v.toFixed(2));

// Pure: daily cost as SVG bars - one series, so no legend; each bar's hover
// target is its whole column and says the day, cost and tokens.
function costChart(days){
  if (!days || !days.length) return "";
  const W = 720, H = 150, L = 44, B = 20, T = 8, n = days.length;
  const top = niceMax(Math.max(0.01, ...days.map(d => d.cost)));
  const slot = (W - L) / n, bw = Math.max(1, slot - 2);
  const y = v => T + (H - T - B) * (1 - v / top);
  const grid = [0, top / 2, top].map(v => `<line class="ax" x1="${L}" x2="${W}" y1="${y(v).toFixed(1)}" y2="${
    y(v).toFixed(1)}"${v ? ' stroke-dasharray="2 3"' : ""}/><text x="${L - 6}" y="${(y(v) + 4).toFixed(1)}" text-anchor="end">$${
    !v || v >= 10 ? Math.round(v) : v.toFixed(1)}</text>`).join("");
  const bars = days.map((d, i) => {
    const x = L + i * slot + 1, top_ = y(d.cost), h = H - B - top_, r = Math.min(4, bw / 2, h);
    const path = h > 0.5 ? `<path class="bar" d="M${x.toFixed(1)},${H - B}V${(top_ + r).toFixed(1)}Q${x.toFixed(1)},${
      top_.toFixed(1)} ${(x + r).toFixed(1)},${top_.toFixed(1)}H${(x + bw - r).toFixed(1)}Q${(x + bw).toFixed(1)},${
      top_.toFixed(1)} ${(x + bw).toFixed(1)},${(top_ + r).toFixed(1)}V${H - B}Z"/>` : "";
    return `<g><title>${esc(dayLabel(d.date) + " · " + dollars(d.cost) + " · " + kfmt(d.tokens) + " tokens")}</title><rect class="hit" x="${
      (L + i * slot).toFixed(1)}" y="${T}" width="${slot.toFixed(1)}" height="${H - T - B}"/>${path}</g>`;
  }).join("");
  // The end labels align to the chart's edges so neither is cut off.
  const xl = [...new Set([0, Math.floor((n - 1) / 2), n - 1])].map(i => {
    const [x, anchor] = i === 0 ? [L, "start"] : i === n - 1 ? [W, "end"] : [L + i * slot + slot / 2, "middle"];
    return `<text x="${x.toFixed(1)}" y="${H - 5}" text-anchor="${anchor}">${esc(dayLabel(days[i].date))}</text>`;
  }).join("");
  return `<svg class="chart" viewBox="0 0 ${W} ${H}" role="img" aria-label="estimated cost per day">${grid}${bars}${xl}</svg>`;
}

async function loadCosts(){
  $("#costs").innerHTML = `<div class="note">reading transcripts…</div>`;
  let d;
  try { d = await get("/api/costs?days=" + ($("#cdays").value || 30)); } catch (e) { return; }
  if (!d.ok){ $("#costs").innerHTML = `<div class="note">${esc(d.error || "unavailable")}</div>`; return; }
  const row = (name, x, extra) => `<tr><td>${esc(name)}</td>${extra || ""}<td class="n">${kfmt(x.tokens)}</td><td class="n">${dollars(x.cost)}</td></tr>`;
  $("#costs").innerHTML = `<div class="kv">${dollars(d.total)} over ${d.window_days} days · ${kfmt(d.total_tokens)} tokens${
      d.complete ? "" : " · still reading older transcripts - reopen for the rest"}</div>
    ${costChart(d.days)}
    <table class="t"><tr><th>project</th><th>sessions</th><th>tokens</th><th>est. cost</th></tr>${
      d.projects.map(p => row(p.project, p, `<td class="n">${p.sessions}</td>`)).join("")}</table>
    <table class="t"><tr><th>model</th><th>tokens</th><th>est. cost</th></tr>${d.models.map(m => row(m.model, m)).join("")}</table>
    <small>What these tokens would cost at API list prices, from the transcripts (subagents included) - an estimate, and
      notional on a subscription. A live session's cost in its status panel is Claude Code's own figure.${
      d.unpriced_tokens ? " " + kfmt(d.unpriced_tokens) + " tokens from models not in the price table are left out." : ""}</small>`;
}

let ticks = 0;
async function tick(){
  try {
    if (!streamUp) gotSessions((await get("/api/sessions")).sessions || []);
    else drawSessions();  // ages on the tiles keep counting between pushes
    if (ticks % 3 === 0) drawMeter();
    if (ticks % 2 === 0) drawPending();
    if (ticks % 20 === 0) loadPrefs();
    loadStatus(false);
    await refreshTab(false);
  } catch (e) { /* dispatcher restarting; the page keeps what it has */ }
  ticks++;
  setTimeout(tick, 1500);
}

async function loadPrefs(){
  try { const r = await get("/api/prefs"); Object.assign(prefs, r.prefs || {}); } catch (e) {}
  $("#group").value = prefs.group || "state";
}

// -- parked permission requests ---------------------------------------
// Only present when policy.json parks `ask` decisions for the window. The
// hook is blocked while one shows, and gives up on its own when it expires.
let pendingSig = "";
async function drawPending(){
  let d;
  try { d = await get("/api/pending"); } catch (e) { return; }
  const items = d.pending || [];
  const sig = JSON.stringify(items.map(i => i.id));
  if (sig === pendingSig) return;
  pendingSig = sig;
  $("#decide").hidden = !items.length;
  $("#decide").innerHTML = items.map(i => `<div class="dq"><span title="${esc(i.input)}"><b>${
    esc(folder(i.cwd))}</b> wants ${esc(i.tool)}: ${esc(i.input)}${i.reason ? " - " + esc(i.reason) : ""}</span>
    <button class="sm" data-dv="allow" data-di="${esc(i.id)}">Allow</button>
    <button class="sm danger" data-dv="deny" data-di="${esc(i.id)}">Deny</button></div>`).join("");
  document.querySelectorAll("[data-dv]").forEach(b => b.onclick = async () => {
    const r = await post("/api/decide", {id: b.dataset.di, verdict: b.dataset.dv, who: "window"});
    $("#say").textContent = r.ok ? b.dataset.dv + " sent" : "too late - it already expired";
    pendingSig = ""; drawPending();
  });
}

// -- usage meter (the drawing is shared with the matrix) ---------------
async function drawMeter(){
  let u;
  try { u = await get("/api/usage"); } catch (e) { return; }
  $("#meter").innerHTML = meterHtml(u);
}

// A click opens the Meter Details and another folds them; remembered here.
let meterOpen = false;
try { meterOpen = localStorage.getItem("ccontrol.meterOpen") === "1"; } catch (e) {}
function foldMeter(open){
  meterOpen = open;
  $("#meter").classList.toggle("open", open);
  $("#meter").setAttribute("aria-expanded", String(open));
  try { localStorage.setItem("ccontrol.meterOpen", open ? "1" : "0"); } catch (e) {}
}
$("#meter").addEventListener("click", () => foldMeter(!meterOpen));
$("#meter").addEventListener("keydown", e => {
  if (e.key === "Enter" || e.key === " ") { e.preventDefault(); foldMeter(!meterOpen); }
});
foldMeter(meterOpen);

// -- tabs ------------------------------------------------------------------
// `view` is what the main column shows: a session (with tabs), history
// search, or insights. `tab` is which face of the session.
let view = "session", tab = "log";

function setView(v){
  view = v;
  $("#sessionpane").hidden = v !== "session";
  $("#historypane").hidden = v !== "history";
  $("#insightpane").hidden = v !== "insights";
  $("#headctl").hidden = v !== "session";
  $("#ask").hidden = v !== "session";
  if (v !== "session") closeMenus();
  if (v === "history"){ $("#title").textContent = "past sessions"; $("#sub").textContent =
      "search every session on this machine - titles, first prompts, and full text"; $("#hq").focus(); search(); }
  if (v === "insights"){ $("#title").textContent = "insights"; $("#sub").textContent =
      "notifications, estimated cost, and tool failures across recent sessions"; loadInsights(); }
  if (v === "session"){ drawHead(); refreshTab(true); }
  document.querySelectorAll("[data-view]").forEach(b => b.classList.toggle("on", b.dataset.view === v));
}

function setTab(t){
  tab = t;
  $("#tabpick").value = t;
  ["log","changes","activity","worktrees"].forEach(n => $("#" + n).hidden = n !== t);
  // Find and prompt jump only mean anything in the conversation.
  $("#findbar").hidden = $("#jump").hidden = t !== "log";
  drawCurrent();
  loadProcs();
  refreshTab(true);
}

let tabAt = 0;
async function refreshTab(force){
  if (view !== "session") return;
  if (tab === "log") return loadLog(force);
  // The other faces cost a git or transcript read; every few seconds is plenty.
  const now = Date.now();
  if (!force && now - tabAt < (tab === "activity" ? 3000 : 6000)) return;
  tabAt = now;
  if (tab === "changes") return loadChanges();
  if (tab === "activity") return loadActivity();
  if (tab === "worktrees") return loadWorktrees();
}

function diffHtml(text){
  return text.split("\n").map(l => {
    const c = l.startsWith("diff --git") ? "f" : l.startsWith("@@") ? "h" :
      l.startsWith("+") && !l.startsWith("+++") ? "a" : l.startsWith("-") && !l.startsWith("---") ? "r" : "";
    return c ? `<span class="${c}">${esc(l)}</span>` : esc(l);
  }).join("\n");
}

async function loadChanges(){
  const s = current();
  if (!s){ $("#changes").innerHTML = `<div class="note">select a session</div>`; return; }
  let d;
  try { d = await get("/api/changes?session=" + encodeURIComponent(key(s))); } catch (e) { return; }
  if (key(s) !== selected || tab !== "changes") return;
  if (!d.ok){ $("#changes").innerHTML = `<div class="note">${esc(d.error)}</div>`; return; }
  if (!d.repo){ $("#changes").innerHTML = `<div class="note">not a git repository</div>`; return; }
  const track = [d.upstream, d.ahead ? d.ahead + " ahead" : "", d.behind ? d.behind + " behind" : ""]
    .filter(Boolean).join(" · ");
  const files = d.files.map(f => `<div><span class="code">${esc(f.code)}</span>${esc(f.path)}${
    f.added != null ? ` <span class="add">+${f.added}</span> <span class="del">-${f.removed}</span>` : ""}</div>`).join("");
  const recent = (d.recent || []).map(c => `<div><span class="code">${esc(c.hash)}</span>${esc(c.subject)} <small>${age(c.when)}</small></div>`).join("");
  const sig = JSON.stringify([d.files, d.diff.length, d.branch, d.recent, reviewSig()]);
  if ($("#changes").dataset.sig === sig) return;
  $("#changes").dataset.sig = sig;
  changesView = {key: key(s), d, track, files, recent};
  drawChanges();
}

// Line comments on the diff: click a line to comment, then send them all to
// the session as one prompt. Kept per session in this browser until sent.
let changesView = null, editing = null;
let reviews = {};
try { reviews = JSON.parse(localStorage.getItem("ccontrol.review") || "{}") || {}; } catch (e) { reviews = {}; }
const saveReviews = () => { try { localStorage.setItem("ccontrol.review", JSON.stringify(reviews)); } catch (e) {} };
// Not the text being typed: a poll must not redraw under the comment box.
const reviewSig = () => JSON.stringify([reviews[selected] || [], editing && [editing.file, editing.line, editing.side]]);
const sameSpot = (a, b) => a.file === b.file && a.line === b.line && a.side === b.side;

function drawChanges(){
  const v = changesView;
  if (!v || v.key !== selected) return;
  const d = v.d, list = reviews[v.key] || [];
  const rows = d.diff ? diffRows(d.diff) : [];
  const placed = new Set();
  const body = rows.map((r, i) => {
    const spot = r.file && r.line ? {file: r.file, line: r.line, side: r.side} : null;
    let html = `<div class="dl${r.cls ? " " + r.cls : ""}"${spot ? ` data-r="${i}" title="click to comment on this line"` : ""}>${esc(r.text) || " "}</div>`;
    if (!spot) return html;
    list.forEach((c, j) => {
      if (!sameSpot(c, spot)) return;
      placed.add(j);
      html += `<div class="rv"><span class="rx" data-rx="${j}" title="remove">✕</span>${esc(c.text)}</div>`;
    });
    if (editing && sameSpot(editing, spot))
      html += `<div class="rv"><textarea id="rvtext" placeholder="comment for the session on this line">${esc(editing.text)}</textarea>
        <div class="btns"><button class="sm" id="rvadd">Add comment</button><button class="sm ghost" id="rvcancel">Cancel</button></div></div>`;
    return html;
  }).join("");
  const orphans = list.map((c, j) => [c, j]).filter(([, j]) => !placed.has(j));
  const review = list.length ? `<div id="review"><b>${plural(list.length, "comment")}</b> on this diff${
      orphans.length ? orphans.map(([c, j]) => `<div class="orph">${esc(c.file)}:${c.line} (no longer in the diff): ${
        esc(c.text)} <span class="rx" data-rx="${j}" style="cursor:pointer">✕</span></div>`).join("") : ""}
    <div class="btns"><button class="sm" id="rvsend" ${current() && !current().remote ? "" : "disabled"}>Send to session</button>
      <button class="sm ghost" id="rvbox">Put in prompt box</button><button class="sm ghost" id="rvclear">Clear</button></div></div>` : "";
  $("#changes").innerHTML = `${review}<div class="kv">branch <b>${esc(d.branch || "?")}</b>${v.track ? " · " + esc(v.track) : ""} · ${
      d.files.length} changed file${d.files.length === 1 ? "" : "s"}${rows.length ? " · click a line in the diff to comment on it" : ""}</div>
    <div class="files">${v.files || "<div class='note'>working tree clean</div>"}</div>
    ${d.diff ? `<div class="diff">${body}${d.truncated ? "\n… (truncated)" : ""}</div>` : ""}
    ${v.recent ? `<div class="kv" style="margin-top:12px">recent commits</div><div class="files">${v.recent}</div>` : ""}`;
  const box = $("#rvtext");
  if (box){ box.focus(); box.setSelectionRange(box.value.length, box.value.length);
    box.oninput = () => { editing.text = box.value; };
    box.onkeydown = e => { if (e.key === "Enter" && (e.ctrlKey || e.metaKey)){ e.preventDefault(); addReview(); }
                           if (e.key === "Escape"){ e.preventDefault(); editing = null; redrawChanges(); } }; }
}

function redrawChanges(){ $("#changes").dataset.sig = ""; drawChanges(); }

function addReview(){
  if (!editing) return;
  const text = editing.text.trim(), k = changesView && changesView.key;
  if (text && k){
    (reviews[k] = reviews[k] || []).push({file: editing.file, line: editing.line, side: editing.side,
      code: editing.code, text});
    saveReviews();
  }
  editing = null;
  redrawChanges();
}

function clearReviews(k){ delete reviews[k]; saveReviews(); redrawChanges(); }

async function sendReview(toBox){
  const s = current(), list = reviews[selected] || [];
  if (!s || !list.length) return;
  const text = reviewPrompt(list);
  if (toBox){
    const box = $("#prompt");
    box.value = (box.value.trim() ? box.value.replace(/\s*$/, "\n\n") : "") + text;
    noteDraft(); clearReviews(key(s)); box.focus();
    return;
  }
  $("#say").textContent = "sending review…";
  const r = await post("/api/dispatch", {session_id: key(s), project: s.cwd, prompt: text, confirm: true, mode: "auto"});
  $("#say").textContent = r.ok ? outcome(r) : (r.error || "refused");
  if (r.ok) clearReviews(key(s));
}

$("#changes").addEventListener("click", e => {
  const t = e.target;
  if (t.closest("#rvadd")) return addReview();
  if (t.closest("#rvcancel")){ editing = null; return redrawChanges(); }
  if (t.closest("#rvsend")) return sendReview(false);
  if (t.closest("#rvbox")) return sendReview(true);
  if (t.closest("#rvclear")) return clearReviews(selected);
  const x = t.closest("[data-rx]");
  if (x){ (reviews[selected] || []).splice(+x.dataset.rx, 1); saveReviews(); return redrawChanges(); }
  const row = t.closest(".dl[data-r]");
  if (!row || !changesView || getSelection().toString()) return;
  const r = diffRows(changesView.d.diff)[+row.dataset.r];
  if (!r) return;
  editing = {file: r.file, line: r.line, side: r.side, code: r.text.slice(1), text: ""};
  redrawChanges();
});

async function loadActivity(){
  const s = current();
  if (!s){ $("#activity").innerHTML = `<div class="note">select a session</div>`; return; }
  let d;
  try { d = await get("/api/activity?session=" + encodeURIComponent(key(s))); } catch (e) { return; }
  if (key(s) !== selected || tab !== "activity") return;
  if (!d.ok){ $("#activity").innerHTML = `<div class="note">${esc(d.error)}</div>`; return; }
  const t = d.tally;
  const tools = Object.entries(t.by_tool).sort((a,b) => b[1][0] - a[1][0])
    .map(([k,[n,e]]) => `${esc(k)} ${n}${e ? ` <span style="color:var(--fail)">(${e} failed)</span>` : ""}`).join(" · ");
  const src = d.source === "hooks" ? "live, from tool hooks" :
    d.source === "conversation" ? "from the conversation (no times - install the tool hooks for live timing)" : (d.note || "");
  const rows = d.timeline.slice().reverse().map(e => {
    const mark = e.ok === true ? `<span class="ok">✓</span>` : e.ok === false ? `<span class="bad">✗</span>` :
      `<span class="run">…</span>`;
    const when = e.start ? clock(e.start) : "";
    const dur = e.start && e.end ? " " + Math.max(0, e.end - e.start).toFixed(1) + "s" : "";
    return `<div class="ev"><span class="t">${when}</span>${mark}<span class="x"><b>${esc(e.tool)}</b> ${esc(e.summary)}${
      esc(dur)}${e.error ? `<span class="err">${esc(e.error)}</span>` : ""}</span></div>`;
  }).join("");
  $("#activity").innerHTML = `<div class="kv">${t.calls} tool calls · ${t.errors} failed · ${esc(src)}</div>
    <div class="kv">${tools}</div>${rows || `<div class="note">no tool calls yet</div>`}`;
}

async function loadWorktrees(){
  const s = current();
  if (!s || s.remote){ $("#worktrees").innerHTML = `<div class="note">${s ? "only for sessions on this machine" : "select a session"}</div>`; return; }
  let d;
  try { d = await get("/api/worktrees?project=" + encodeURIComponent(s.cwd || "")); } catch (e) { return; }
  if (key(s) !== selected || tab !== "worktrees") return;
  if (!d.ok){ $("#worktrees").innerHTML = `<div class="note">${esc(d.error)}</div>`; return; }
  if (!d.repo){ $("#worktrees").innerHTML = `<div class="note">not a git repository</div>`; return; }
  const cards = d.worktrees.map(w => `<div class="card">
      <h3>${esc(w.branch || "(detached)")}</h3>
      <small>${esc(w.path)}</small>
      <div class="kv" style="margin:4px 0 0">${w.agent ? "background session" : "made by hand - not managed here"} · ${
        w.ahead || 0} ahead of ${esc(d.base)}${w.behind ? " · " + w.behind + " behind" : ""}${w.dirty ? " · <b>uncommitted changes</b>" : ""}${
        w.subject ? " · last: " + esc(w.subject) : ""}</div>
      <div class="btns"><button class="sm ghost" data-wd="${esc(w.path)}">Diff</button>${w.agent ? `
        <button class="sm" data-wa="merge" data-wp="${esc(w.path)}" ${w.ahead && !w.dirty ? "" : "disabled"}>Merge into ${esc(d.base)}</button>
        <button class="sm ghost" data-wa="pr" data-wp="${esc(w.path)}" ${w.dirty ? "disabled" : ""}>Push + open PR</button>
        <button class="sm danger" data-wa="discard" data-wp="${esc(w.path)}">Discard</button>` : ""}</div>
      <div class="wdiff" data-for="${esc(w.path)}"></div></div>`).join("");
  const sig = JSON.stringify(d.worktrees);
  if ($("#worktrees").dataset.sig === sig) return;
  $("#worktrees").dataset.sig = sig;
  $("#worktrees").innerHTML = `<div class="kv">worktrees of ${esc(d.main)} (base ${esc(d.base)})</div>${
    cards || `<div class="note">no linked worktrees - background sessions leave theirs under .claude/worktrees</div>`}`;
  document.querySelectorAll("[data-wd]").forEach(b => b.onclick = async () => {
    const r = await get("/api/worktree_diff?project=" + encodeURIComponent(s.cwd) + "&path=" + encodeURIComponent(b.dataset.wd));
    const box = [...document.querySelectorAll(".wdiff")].find(x => x.dataset.for === b.dataset.wd);
    box.innerHTML = r.ok ? `<pre class="files">${esc(r.log)}</pre><div class="diff">${diffHtml(r.diff)}${r.truncated ? "\n… (truncated)" : ""}</div>`
      : `<div class="note">${esc(r.error)}</div>`;
  });
  document.querySelectorAll("[data-wa]").forEach(b => b.onclick = () => finishWorktree(s, b.dataset.wa, b.dataset.wp));
}

async function finishWorktree(s, action, path, force){
  const say = {merge: "Merge this branch into the main checkout?",
               pr: "Push this branch to origin and open a pull request?",
               discard: "Remove this worktree and delete its branch?"}[action];
  if (!force && !confirm(say)) return;
  const r = await post("/api/worktree", {project: s.cwd, path, action, force: !!force});
  if (!r.ok && r.needs_force){
    if (confirm(r.error + ". Discard anyway?")) return finishWorktree(s, action, path, true);
    return;
  }
  $("#say").textContent = r.ok ? (action === "pr" && r.url ? "opened " + r.url : action + " done: " + (r.branch || ""))
    : (r.error || "refused");
  $("#worktrees").dataset.sig = ""; loadWorktrees();
}

// The tile's ✕: ends the session, and with it its terminal tab.
async function closeSession(k){
  const s = sessions.find(x => key(x) === k);
  if (!s || !confirm("Close " + nameOf(s) + " and its window? The conversation stays resumable." +
      (s.status === "busy" ? "\n\nIt is working right now - its turn will be interrupted." : ""))) return;
  $("#say").textContent = "closing " + nameOf(s) + "…";
  const r = await post("/api/close", {session_id: k});
  $("#say").textContent = !r.ok ? "could not close " + nameOf(s) + ": " + (r.error || "refused")
    : r.how === "killed" ? nameOf(s) + " had to be ended forcibly, so its tab is still open"
    : r.how === "exit" && r.tab_closed === false ? nameOf(s) + " has exited; its tab is still open"
    : "closed " + nameOf(s);
}

// -- resume, fork --------------------------------------------------------
async function act(what){
  const s = current();
  if (!s) return;
  if (what === "copy"){
    try { await navigator.clipboard.writeText(s.resume_command + (s.cwd ? "" : "")); $("#say").textContent = "copied: " + s.resume_command; }
    catch (e) { $("#say").textContent = s.resume_command; }
    return;
  }
  if (what === "show"){
    const r = await post("/api/focus", {session_id: key(s)});
    $("#say").textContent = r.ok ? (r.note || "raised its window") : (r.error || "refused");
    return;
  }
  if (what === "new" || what === "new-close"){
    const close = what === "new-close";
    if (close && !confirm("Open a new session in " + (s.cwd || "this project") + " and close " + nameOf(s) + "?" +
        (s.status === "busy" ? "\n\nIt is working right now - its turn will be interrupted." : ""))) return;
    $("#say").textContent = close ? "opening a new session and closing this one…" : "opening a new session…";
    const r = await post("/api/new", {session_id: key(s), cwd: s.cwd || null, close});
    if (!r.ok){ $("#say").textContent = r.error || "refused"; return; }
    follow(r.session_id);
    const c = r.closed;
    $("#say").textContent = "opened a new session in " + (s.cwd || "its project") + " - selecting it once it starts" +
      (!c ? "" : !c.ok ? "; could not close the old one: " + (c.error || "refused")
        : c.how === "killed" ? "; the old one had to be ended forcibly, so its tab is still open"
        : c.tab_closed === false && c.how !== "gone" ? "; the old one has exited, its tab is still open" : "; the old one is closed");
    return;
  }
  const [kind, where] = what.split("-");
  const r = await post("/api/resume", {session_id: key(s), where, fork: kind === "fork",
    prompt: $("#prompt").value.trim() || null});
  $("#say").textContent = r.ok ? (r.where === "background" ? kind + "ed in the background · " + (r.id || "")
    : "opened a new tab: " + r.command) : (r.error || "refused");
  if (r.ok && where === "background") clearPrompt(key(s));
}

// A session launched from here is selected once it shows up on the board -
// it takes a few seconds to start. Given up after a minute, so a launch that
// never came up does not yank the view away later.
let following = null;
function follow(id){ following = id ? {id, until: Date.now() + 60000} : null; }
function catchFollowed(){
  if (!following) return;
  if (Date.now() > following.until){ following = null; return; }
  const s = sessions.find(x => key(x) === following.id);
  if (!s) return;
  following = null;
  select(key(s));
}

// Any other new session - started from a terminal, the project picker, a
// fork - is switched to as well, unless you are mid-sentence in the prompt
// box, where a switch would leave your next keystrokes aimed at the wrong
// session; then it is only announced.
function catchStarted(k){
  if (!k || k === selected) return;
  const box = $("#prompt");
  if (document.activeElement === box && box.value.trim()){
    const s = sessions.find(x => key(x) === k);
    $("#say").textContent = "new session " + (s ? nameOf(s) : k) + " started - not switched while you type";
    return;
  }
  pick(k);
}

function select(id){
  selected = id;
  if (view !== "session") setView("session");
  showCached(); drawSessions(); refreshTab(true); loadStatus(true); loadProcs();
}

// -- queue -----------------------------------------------------------------
async function queue(){
  const s = current();
  const text = $("#prompt").value.trim();
  if (!s || !text) return;
  const r = await post("/api/queue", {session_id: key(s), prompt: text});
  if (r.ok){ clearPrompt(key(s)); $("#say").textContent = r.waiting ?
      "queued - delivered when its current turn ends (" + r.waiting + " waiting)" : "delivered now - it was idle"; }
  else $("#say").textContent = r.error || "refused";
  queuedSig = "";
}

let queuedSig = "";
async function drawQueued(s){
  if (!s || !s.queued){ $("#queued").innerHTML = ""; queuedSig = ""; return; }
  let d;
  try { d = await get("/api/queue"); } catch (e) { return; }
  const items = (d.queue || {})[pkey(s)] || [];
  const sig = JSON.stringify(items);
  if (sig === queuedSig) return;
  queuedSig = sig;
  $("#queued").innerHTML = items.map(i => `<div class="qi${i.error ? " err" : ""}"><span title="${esc(i.prompt)}">queued: ${
    esc(i.prompt)}${i.error ? " - last try: " + esc(i.error) : ""}</span><button class="tiny" data-uq="${esc(i.id)}">cancel</button></div>`).join("");
  document.querySelectorAll("[data-uq]").forEach(b => b.onclick = async () => {
    await post("/api/unqueue", {session: pkey(s), id: b.dataset.uq}); queuedSig = ""; });
}

// -- history search --------------------------------------------------------
let searchSeq = 0;
async function search(){
  const q = $("#hq").value.trim();
  const seq = ++searchSeq;
  $("#hits").innerHTML = `<div class="note">searching…</div>`;
  let d;
  try { d = await get("/api/history?q=" + encodeURIComponent(q)); } catch (e) { return; }
  if (seq !== searchSeq) return;
  if (!d.ok){ $("#hits").innerHTML = `<div class="note">${esc(d.error)}</div>`; return; }
  $("#hits").innerHTML = (d.results.map(r => `<div class="hit">
      <div class="tt">${esc(r.title || r.first_prompt || r.session_id)}</div>
      <div class="sn">${esc(folder(r.cwd))} · ${esc(new Date(r.modified*1000).toLocaleString())}${
        r.branch ? " · " + esc(r.branch) : ""}${r.live ? " · <b>live</b>" : ""}${r.where ? " · matched in " + esc(r.where) : ""}</div>
      ${r.snippet ? `<div class="sn">${esc(r.snippet)}</div>` : ""}
      <div class="btns">${r.live ? `<button class="sm" data-open="${esc(r.session_id)}">Open</button>` :
        `<button class="sm" data-rs="${esc(r.session_id)}" data-w="window">Resume</button>
         <button class="sm ghost" data-rs="${esc(r.session_id)}" data-w="background">Resume in bg</button>`}
        <button class="sm ghost" data-rs="${esc(r.session_id)}" data-w="window" data-fork="1">Fork</button>
        <button class="sm ghost" data-cp="claude --resume ${esc(r.session_id)}">Copy cmd</button></div>
    </div>`).join("") || `<div class="note">nothing found</div>`) +
    (d.complete ? "" : `<div class="note">still indexing older sessions - search again for complete results</div>`);
  document.querySelectorAll("[data-open]").forEach(b => b.onclick = () => {
    selected = b.dataset.open; setView("session"); });
  document.querySelectorAll("[data-rs]").forEach(b => b.onclick = async () => {
    const r = await post("/api/resume", {session_id: b.dataset.rs, where: b.dataset.w, fork: !!b.dataset.fork});
    b.parentNode.insertAdjacentHTML("beforeend", `<small>${esc(r.ok ? (r.id ? "started " + r.id : "opened") : r.error)}</small>`);
  });
  document.querySelectorAll("[data-cp]").forEach(b => b.onclick = async () => {
    try { await navigator.clipboard.writeText(b.dataset.cp); b.textContent = "copied"; } catch (e) { b.textContent = b.dataset.cp; } });
}

// -- insights: failures and notifications ------------------------------
async function loadInsights(){
  $("#errs").innerHTML = `<div class="note">reading recent sessions…</div>`;
  let n;
  try { n = await get("/api/notify"); } catch (e) { n = null; }
  if (n) $("#notif").innerHTML = `
    <label class="sw"><input type="checkbox" data-ch="speak" ${n.speak.enabled ? "checked" : ""}> Say it out loud when a session needs you (after ${n.speak.delay}s)</label>
    <label class="sw"><input type="checkbox" data-ch="ntfy" ${n.ntfy.enabled ? "checked" : ""} ${n.ntfy.configured ? "" : "disabled"}> Push to phone via ntfy (after ${n.ntfy.delay}s)${
      n.ntfy.configured ? "" : " - set notify.ntfy.topic in config.json first"}</label>
    ${n.last_error ? `<div class="note" style="text-align:left;padding:4px 0">last error: ${esc(n.last_error)}</div>` : ""}`;
  document.querySelectorAll("[data-ch]").forEach(c => c.onchange = () =>
    post("/api/notify", {channel: c.dataset.ch, enabled: c.checked}));
  // This one belongs to the browser, not the dispatcher: each window asks
  // for its own permission to show notifications.
  if ("Notification" in window){
    $("#notif").insertAdjacentHTML("beforeend", `<label class="sw"><input type="checkbox" id="desk" ${
      deskNotify && Notification.permission === "granted" ? "checked" : ""}> Desktop notification from this window when a session starts waiting for you (only while the window is not in front)</label>`);
    $("#desk").onchange = async e => {
      const box = e.target;
      if (box.checked && Notification.permission !== "granted"){
        const p = await Notification.requestPermission();
        if (p !== "granted"){ box.checked = false; $("#say").textContent = "the browser did not allow notifications (" + p + ")"; }
      }
      deskNotify = box.checked;
      try { localStorage.setItem("ccontrol.desknotify", deskNotify ? "1" : ""); } catch (x) {}
    };
  }
  loadCosts();
  let d;
  try { d = await get("/api/errors?days=" + ($("#days").value || 7)); } catch (e) { return; }
  if (!d.ok){ $("#errs").innerHTML = `<div class="note">${esc(d.error)}</div>`; return; }
  const rate = (e, n) => n ? (100 * e / n).toFixed(1) + "%" : "-";
  const cats = Object.entries(d.by_category).sort((a,b) => b[1] - a[1]);
  const tools = Object.entries(d.by_tool).filter(([,v]) => v[1]).sort((a,b) => b[1][1] - a[1][1]);
  const projs = Object.entries(d.by_project).filter(([,v]) => v.calls).sort((a,b) => b[1].errors - a[1].errors);
  $("#errs").innerHTML = `<div class="kv">${d.sessions} sessions · ${d.calls} tool calls · ${d.errors} failed (${rate(d.errors, d.calls)})</div>
    <table class="t"><tr><th>why it failed</th><th>count</th><th>example</th></tr>${cats.map(([c,n]) =>
      `<tr><td>${esc(c)}</td><td class="n">${n}</td><td><small>${esc((d.examples[c] || "").slice(0,140))}</small></td></tr>`).join("")}</table>
    <table class="t"><tr><th>tool</th><th>failed</th><th>calls</th><th>rate</th></tr>${tools.map(([t,[n,e]]) =>
      `<tr><td>${esc(t)}</td><td class="n">${e}</td><td class="n">${n}</td><td class="n">${rate(e,n)}</td></tr>`).join("")}</table>
    <table class="t"><tr><th>project</th><th>sessions</th><th>failed</th><th>calls</th><th>rate</th></tr>${projs.map(([p,v]) =>
      `<tr><td>${esc(p)}</td><td class="n">${v.sessions}</td><td class="n">${v.errors}</td><td class="n">${v.calls}</td><td class="n">${rate(v.errors,v.calls)}</td></tr>`).join("")}</table>`;
}

// What a successful dispatch actually did. "inject" alone read as "done", but
// it only means the session's inbox took the message: Claude Code's own
// inbound policy still decides, and on the Pi it once held one out of sight.
function outcome(r){
  const where = r.host ? " on " + r.host : "";
  const id = r.bg_id ? " · " + r.bg_id : "";
  if (r.action === "inject")
    return "delivered to the inbox of " + (r.session_name || "the session") +
           where + " - it runs once the session accepts it";
  if (r.action === "remote") return "started in the background" + where + id;
  if (r.action === "background") return "started in the background" + id;
  if (r.action === "launch") return "opened a new session window";
  if (r.action === "typed")
    return "typed into " + (r.session_name || "the session") + "'s prompt" +
           (r.cleaned ? " (made one line)" : "");
  if (r.action === "command")
    return "typed " + r.command + " into " + (r.session_name || "the session") + "'s terminal";
  return r.action + id;
}

async function send(mode){
  const s = sessions.find(x => key(x) === selected);
  const text = $("#prompt").value.trim();
  if (!s || !text || busy) return;
  busy = true; $("#say").textContent = "sending…";
  const r = await post("/api/dispatch", {
    session_id: key(s), project: s.cwd, prompt: text, confirm: true, mode: mode || "auto"
  });
  busy = false;
  if (r.ok){ clearPrompt(key(s)); $("#say").textContent = outcome(r); }
  else $("#say").textContent = r.error || "refused";
  lastSig = ""; loadLog(true);
}

// Typed at the session's prompt, so it counts as you there - what a sent
// message cannot (see Control.send_typed).
async function sendTyped(){
  closeMenus();
  const s = sessions.find(x => key(x) === selected);
  const text = $("#prompt").value.trim();
  if (!s || !text || busy) return;
  busy = true; $("#say").textContent = "typing…";
  const r = await post("/api/typed", {session_id: key(s), prompt: text});
  busy = false;
  if (r.ok){ clearPrompt(key(s)); $("#say").textContent = outcome(r); }
  else $("#say").textContent = r.error || "refused";
  lastSig = ""; loadLog(true);
}

// -- dictation ----------------------------------------------------------
// The mic lives here, beside the box it fills. The browser records (localhost
// is a secure context) and the dispatcher transcribes on the model it already
// holds - loading costs seconds, using it costs hundredths.
let rec = null, chunks = [];

// " on CPU (small)" when the dictation process has fallen back to the CPU,
// which is why a dictation is slow there; "" on a GPU or before it is known.
const onCpu = r => r && r.device === "cpu" ? " on CPU" + (r.model ? " (" + r.model + ")" : "") : "";

async function dictate(){
  if (rec && rec.state === "recording"){ rec.stop(); return; }
  let stream;
  try { stream = await navigator.mediaDevices.getUserMedia({audio:true}); }
  catch (e) { $("#say").textContent = "no microphone: " + e.name; return; }
  chunks = [];
  let cpuAt = null;  // the warm-up's answer: where the model runs
  rec = new MediaRecorder(stream);
  rec.ondataavailable = e => { if (e.data.size) chunks.push(e.data); };
  rec.onstop = async () => {
    stream.getTracks().forEach(t => t.stop());
    $("#mic").classList.remove("rec");
    $("#mic").textContent = "\u{1F3A4} Dictate";
    if (!chunks.length) return;
    $("#say").textContent = "transcribing" + onCpu(cpuAt) + "\u2026";
    const r = await fetch("/api/transcribe", {method:"POST", cache:"no-store",
      headers:{"Content-Type":"application/octet-stream"},
      body: new Blob(chunks)});
    const data = await r.json();
    if (!data.ok){ $("#say").textContent = data.error || "transcription failed"; return; }
    if (!data.text){ $("#say").textContent = "nothing heard"; return; }
    const box = $("#prompt");
    box.value = (box.value.trim() ? box.value.replace(/\s*$/, " ") : "") + data.text;
    box.focus();
    noteDraft();
    $("#say").textContent = "dictated " + data.text.length + " chars" + onCpu(data);
  };
  rec.start();
  // The model lives in its own process, ended when idle; start it now so it
  // is built while you speak rather than after.
  post("/api/voice/warm", {}).then(r => {
    cpuAt = r;
    if (rec && rec.state === "recording" && onCpu(r)) $("#say").textContent = "listening" + onCpu(r) + "\u2026";
  }).catch(() => {});
  $("#mic").classList.add("rec");
  $("#mic").textContent = "\u25A0 Stop";
  $("#say").textContent = "listening\u2026";
}

// -- starting a project that has no session -----------------------------
// The sidebar lists sessions, so without this a project nobody is currently
// working in would be unreachable - which the old palette could do.
let projects = [], psel = 0;

function drawProjects(){
  const q = $("#find").value.trim().toLowerCase();
  const norm = s => String(s).toLowerCase().replace(/[-_\/]+/g," ");
  const hit = projects.filter(p => !q || norm(p.name).includes(q) ||
                                   norm(p.name).replace(/ /g,"").includes(q.replace(/ /g,"")))
                      .slice(0, 40);
  psel = Math.min(psel, Math.max(0, hit.length - 1));
  $("#projects").innerHTML = hit.map((p,i) =>
    `<div class="p${i===psel?" on":""}" data-n="${esc(p.name)}">${esc(p.name)}
      <small>${esc(p.path)}</small></div>`).join("") ||
    `<div class="note">no match</div>`;
  document.querySelectorAll(".p").forEach((el,i) => {
    el.onclick = () => { psel = i; startProject(el.dataset.n); };
  });
  return hit;
}

async function startProject(name){
  const text = $("#prompt").value.trim();
  $("#say").textContent = "starting " + name + "\u2026";
  const r = await post("/api/dispatch", {
    project: name, prompt: text || "Ready when you are.", confirm: true,
    mode: text ? "auto" : "launch"
  });
  $("#say").textContent = r.ok ? outcome(r) : (r.error || "refused");
  if (r.ok){ clearPrompt(); togglePicker(false); }
}

function togglePicker(show){
  const open = show === undefined ? $("#picker").hidden : show;
  $("#picker").hidden = !open;
  if (open){ $("#find").value = ""; psel = 0; drawProjects(); $("#find").focus(); }
}

// -- chat matrix ---------------------------------------------------------
// Its own window, so it can sit on another screen. The matrix page says it is
// up once a second; the button closes it by asking it to close itself, which
// still works after this page reloads and loses the handle it opened it with.
const MATRIX = "imperatorium-matrix";
const matrixChannel = "BroadcastChannel" in window ? new BroadcastChannel(MATRIX) : null;
let matrixWin = null, matrixSeen = 0;

function matrixUp(){
  return (matrixWin && !matrixWin.closed) || Date.now() - matrixSeen < 2500;
}

function showMatrixButton(){
  const b = $("#matrixbtn"), up = matrixUp();
  b.classList.toggle("on", up);
  b.setAttribute("aria-pressed", up ? "true" : "false");
}

// A double click on a tile asks for its session here. This window answers
// so the matrix knows not to open another one.
if (matrixChannel) matrixChannel.onmessage = e => {
  const m = e.data;
  if (m && m.select){
    matrixChannel.postMessage({selected: m.select});
    if (sessions.some(x => key(x) === m.select)) select(m.select); else follow(m.select);
    window.focus();
    return;
  }
  if (m === "open" || m === "closed"){
    matrixSeen = m === "open" ? Date.now() : 0;
    showMatrixButton();
  }
};

function toggleMatrix(){
  if (matrixUp()){
    if (matrixWin && !matrixWin.closed) matrixWin.close();
    if (matrixChannel) matrixChannel.postMessage("close");
    matrixWin = null; matrixSeen = 0;
  } else {
    const w = Math.round(screen.availWidth * .85), h = Math.round(screen.availHeight * .85);
    matrixWin = window.open("/matrix", MATRIX, `popup,width=${w},height=${h}`);
    if (!matrixWin) $("#say").textContent = "the browser blocked the Chat Matrix window - allow pop-ups for this page";
  }
  showMatrixButton();
}
$("#matrixbtn").onclick = toggleMatrix;

// -- settings ----------------------------------------------------------------
// Its own window, like the Chat Matrix; a second click brings it forward.
$("#setbtn").onclick = () => {
  const w = window.open("/settings", "imperatorium-settings", "popup,width=760,height=820");
  if (!w) $("#say").textContent = "the browser blocked the Settings window - allow pop-ups for this page";
  else w.focus();
};
setInterval(showMatrixButton, 1000);

$("#new").onclick = async () => {
  if ($("#picker").hidden){
    try { projects = (await get("/api/projects")).projects || []; } catch(e){}
  }
  togglePicker();
};
$("#find").addEventListener("input", () => { psel = 0; drawProjects(); });
$("#find").addEventListener("keydown", e => {
  const hit = drawProjects();
  if (e.key === "ArrowDown"){ e.preventDefault(); psel = Math.min(psel+1, hit.length-1); drawProjects(); }
  else if (e.key === "ArrowUp"){ e.preventDefault(); psel = Math.max(psel-1, 0); drawProjects(); }
  else if (e.key === "Enter"){ e.preventDefault(); if (hit[psel]) startProject(hit[psel].name); }
  else if (e.key === "Escape"){ e.preventDefault(); togglePicker(false); }
});

$("#mic").onclick = dictate;
document.addEventListener("keydown", e => {
  if (e.key === "F9"){ e.preventDefault(); dictate(); }
});

$("#send").onclick = () => send("auto");
$("#sendtyped").onclick = sendTyped;
$("#bg").onclick = () => send("background");
$("#queue").onclick = queue;
$("#prompt").addEventListener("keydown", e => {
  if (quick.key && comboOf(e) === quick.key){ e.preventDefault(); openQuick(true); return; }
  if (e.key === "Enter" && e.altKey){ e.preventDefault(); queue(); return; }
  if (e.key === "Enter" && !e.shiftKey){ e.preventDefault(); send(e.ctrlKey ? "background" : "auto"); }
});
$("#filter").addEventListener("input", drawSessions);
$("#group").onchange = () => savePrefs({group: $("#group").value});
$("#tabpick").onchange = () => setTab($("#tabpick").value);
$("#curbtn").onclick = () => { closeMenus(); showCurrent = !showCurrent; currentFolded = false; drawCurrent(); };
// The header's menus: one open at a time, shut by a click elsewhere or Esc.
// A change inside one (a display shape) leaves it open.
function closeMenus(except){
  document.querySelectorAll("details.menu[open]").forEach(d => { if (d !== except) d.open = false; });
}
document.querySelectorAll("details.menu").forEach(d =>
  d.addEventListener("toggle", () => { if (d.open) closeMenus(d); }));
document.addEventListener("click", e => {
  if (!(e.target.closest && e.target.closest("details.menu"))) closeMenus();
});
document.addEventListener("keydown", e => {
  if (e.key === "Escape" && document.querySelector("details.menu[open]")){ closeMenus(); e.preventDefault(); }
});
$("#curhd").onclick = () => { currentFolded = !currentFolded; drawCurrent(); };
$("#jprev").onclick = () => jump(-1);
$("#status").addEventListener("click", e => {
  if (!e.target.closest(".hd")) return;
  statusFolded = !statusFolded;
  try { localStorage.setItem("ccontrol.statusFolded", statusFolded ? "1" : ""); } catch (e) {}
  drawStatus();
});
$("#jnext").onclick = () => jump(1);
drawShape();
$("#shape").addEventListener("change", e => {
  const cat = e.target.dataset.shape;
  if (!cat) return;
  if (e.target.value === "full") delete shape[cat]; else shape[cat] = e.target.value;
  try { localStorage.setItem("ccontrol.shape", JSON.stringify(shape)); } catch (e) {}
  drawShape();
  drawLog();
});
$("#log").addEventListener("click", e => {
  const b = e.target.closest && e.target.closest("[data-earlier]");
  if (b) showEarlier(b.dataset.earlier === "all");
  if (e.target.closest && e.target.closest("[data-newest]")) showNewest();
  const c = e.target.closest && e.target.closest("[data-coldact]");
  if (c) act(c.dataset.coldact);
});
// A scroll is the reader's, or ours to the end (which lands at the end, so
// it keeps following). Content growing does not fire one.
$("#log").addEventListener("scroll", () => {
  logFollow = logAtEnd($("#log"));
  markAway();
}, {passive: true});
// The log resized - the drawer, status panel or prompt box changed, the
// window did, or the tab came back into view: stay at the end if following.
new ResizeObserver(() => { if (logFollow) showNewest(); else markAway(); }).observe($("#log"));
if (document.fonts) document.fonts.ready.then(() => { if (logFollow) showNewest(); });
// `toggle` does not bubble; listen on the way down.
$("#log").addEventListener("toggle", e => {
  const d = e.target;
  if (!d.matches || !d.matches("details.cmp")) return;
  if (d.open) opened.add(d.dataset.k); else opened.delete(d.dataset.k);
  // Opening a block is not new text: the view stays put, and if that pushed
  // the end out of view the log stops following, as if scrolled.
  logFollow = logAtEnd($("#log"));
  markAway();
}, true);
document.addEventListener("keydown", e => {
  if (!e.altKey || view !== "session" || tab !== "log") return;
  if (e.key === "ArrowUp"){ e.preventDefault(); jump(-1); }
  if (e.key === "ArrowDown"){ e.preventDefault(); jump(1); }
});
$("#prompt").addEventListener("input", noteDraft);
document.querySelectorAll("[data-view]").forEach(b => b.onclick = () =>
  setView(view === b.dataset.view ? "session" : b.dataset.view));
let hqTimer;
$("#hq").addEventListener("input", () => { clearTimeout(hqTimer); hqTimer = setTimeout(search, 350); });
$("#days").onchange = loadInsights;
// Named, so the matrix can find this window again; opened by the matrix to
// show one session, it selects that session once it is listed.
window.name = "imperatorium-main";
{
  const asked = new URLSearchParams(location.search).get("session");
  if (asked){ follow(asked); history.replaceState(null, "", location.pathname); }
}

// -- quick phrases -----------------------------------------------------------
// The Settings window's phrase list, opened from the prompt box by its own
// shortcut. Read when opened, so a change in Settings needs no reload.
let quick = {key: "ctrl+space", phrases: []}, qsel = 0;
async function loadQuick(){
  try { const d = await get("/api/quick"); if (d.ok) quick = d; } catch (e) {}
}
function quickHits(){
  const q = $("#qq").value.trim().toLowerCase();
  return (quick.phrases || []).map((p, i) => ({p, i}))
    .filter(h => !q || h.p.toLowerCase().includes(q));
}
function drawQuick(){
  const hit = quickHits();
  qsel = Math.max(0, Math.min(qsel, hit.length - 1));
  $("#qlist").innerHTML = hit.map((h, n) =>
    `<div class="ji${n === qsel ? " on" : ""}" data-qi="${h.i}"><span class=qn>${h.i < 9 ? h.i + 1 : ""}</span>` +
    `<span>${esc(h.p)}</span></div>`).join("") ||
    `<div class=note>${(quick.phrases || []).length ? "no match"
      : "No phrases yet - add them in Settings, under Quick phrases."}</div>`;
  const on = $("#qlist .ji.on");
  if (on) on.scrollIntoView({block: "nearest"});
}
async function openQuick(show){
  $("#quickbox").hidden = !show;
  if (!show){ $("#prompt").focus(); return; }
  $("#qq").value = ""; qsel = 0; drawQuick(); $("#qq").focus();
  await loadQuick(); drawQuick();
}
// Into the prompt box at the cursor, replacing a selection, with a space
// either side where it would otherwise run into the text around it.
function takeQuick(i){
  const phrase = (quick.phrases || [])[i];
  openQuick(false);
  if (phrase == null) return;
  const box = $("#prompt"), v = box.value;
  const a = box.selectionStart ?? v.length, b = box.selectionEnd ?? v.length;
  const before = a && !/\s$/.test(v.slice(0, a)) ? " " : "";
  const after = b < v.length && !/^\s/.test(v.slice(b)) ? " " : "";
  box.setRangeText(before + phrase + after, a, b, "end");
  box.focus();
  noteDraft();
}
$("#qq").addEventListener("input", () => { qsel = 0; drawQuick(); });
$("#qq").addEventListener("keydown", e => {
  const hit = quickHits();
  if (e.key === "ArrowDown"){ e.preventDefault(); qsel = Math.min(qsel + 1, hit.length - 1); drawQuick(); }
  else if (e.key === "ArrowUp"){ e.preventDefault(); qsel = Math.max(qsel - 1, 0); drawQuick(); }
  else if (e.key === "Enter"){ e.preventDefault(); if (hit[qsel]) takeQuick(hit[qsel].i); }
  else if (e.key === "Escape" || comboOf(e) === quick.key){ e.preventDefault(); openQuick(false); }
  // A digit picks by number until something has been typed to filter with.
  else if (/^[1-9]$/.test(e.key) && !$("#qq").value && !e.ctrlKey && !e.altKey && !e.metaKey){
    e.preventDefault(); if (+e.key <= (quick.phrases || []).length) takeQuick(+e.key - 1);
  }
});
$("#qlist").addEventListener("click", e => {
  const el = e.target.closest("[data-qi]");
  if (el) takeQuick(+el.dataset.qi);
});
$("#quickbox").addEventListener("click", e => { if (e.target.id === "quickbox") openQuick(false); });
loadQuick();

$("#jq").addEventListener("input", () => { jsel = 0; drawJump(); });
$("#jq").addEventListener("keydown", e => {
  const hit = jumpHits();
  if (e.key === "ArrowDown"){ e.preventDefault(); jsel = Math.min(jsel + 1, hit.length - 1); drawJump(); }
  else if (e.key === "ArrowUp"){ e.preventDefault(); jsel = Math.max(jsel - 1, 0); drawJump(); }
  else if (e.key === "Enter"){ e.preventDefault(); const s = hit[jsel]; openJump(false); if (s) pick(key(s)); }
  else if (e.key === "Escape"){ e.preventDefault(); openJump(false); }
});
$("#jlist").addEventListener("click", e => {
  const el = e.target.closest("[data-jk]");
  if (el){ openJump(false); pick(el.dataset.jk); }
});
$("#jumpbox").addEventListener("click", e => { if (e.target.id === "jumpbox") openJump(false); });
document.addEventListener("keydown", e => {
  const k = (e.key || "").toLowerCase(), ctl = (e.ctrlKey || e.metaKey) && !e.altKey;
  if (ctl && k === "k"){ e.preventDefault(); openJump($("#jumpbox").hidden); return; }
  if (ctl && k === "f" && view === "session" && tab === "log"){ e.preventDefault(); $("#cfind").focus(); $("#cfind").select(); return; }
  if (!$("#jumpbox").hidden) return;
  if (e.altKey && !e.ctrlKey && /^[1-9]$/.test(e.key)){ e.preventDefault(); pick(visibleKeys()[+e.key - 1]); return; }
  if (e.altKey && !e.ctrlKey && k === "n"){ e.preventDefault(); goWaiting(); return; }
  if (typing(e)){ if (e.key === "Escape" && e.target.id === "prompt") e.target.blur(); return; }
  if (e.ctrlKey || e.altKey || e.metaKey) return;
  if (k === "j"){ e.preventDefault(); stepSession(1); }
  else if (k === "k"){ e.preventDefault(); stepSession(-1); }
  else if (k === "n"){ e.preventDefault(); goWaiting(); }
  else if (e.key === "/"){ e.preventDefault(); $("#filter").focus(); }
});

let findTimer;
$("#cfind").addEventListener("input", () => { clearTimeout(findTimer); findTimer = setTimeout(runFind, 200); });
$("#cfilter").onchange = runFind;
$("#cfind").addEventListener("keydown", e => {
  if (e.key === "Enter"){ e.preventDefault(); goFind(e.shiftKey ? 1 : -1); }
  if (e.key === "Escape"){ e.preventDefault(); resetFind(); drawLog(); $("#cfind").blur(); }
});
$("#cfprev").onclick = () => goFind(-1);
$("#cfnext").onclick = () => goFind(1);
$("#cdays").onchange = loadCosts;

loadPrefs().then(tick);
openStream();
procLoop();
"""


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
    ("Window", "F9", "dictate into the prompt box; again to stop"),
    ("Window", "Esc", "close an open menu"),
    ("Conversation", "Ctrl+F", "find in the conversation"),
    ("Conversation", "Enter / Shift+Enter", "in find: earlier / later match"),
    ("Conversation", "Alt+Up / Alt+Down", "your previous / next prompt"),
    ("Lists", "Up / Down, Enter, Esc", "session jump, project picker and quick phrases: "
                                       "choose, take, close"),
    ("Changes", "Ctrl+Enter, Esc", "review comment: add, cancel"),
    ("Settings", "Ctrl+S", "save"),
]


def _with_meta(token, page):
    """`page` carrying the window token (see Control.send_typed)."""
    head = "<meta charset=utf-8>"
    return page.replace(head, head + '<meta name=imp-window content="%s">'
                        % html.escape(token or "", quote=True), 1)


def render_shell(token=""):
    return _with_meta(token, (
        "<!doctype html><html lang=en><head><meta charset=utf-8>"
        '<meta name=viewport content="width=device-width,initial-scale=1,viewport-fit=cover">'
        "<title>Imperatorium</title><link id=fav rel=icon href=\"data:,\">"
        "<style>%s</style></head><body>"
        '<div id=app>'
        '<div id=side><h1><span class=ttl>'
        '<button id=setbtn class="tiny icon" title="Settings" aria-label="Settings">'
        '<svg viewBox="0 0 16 16" width=14 height=14 aria-hidden=true><circle cx=8 cy=8 r=2.1 /><path d="M6.9 1.5h2.2l.4 1.9 1.3.6 1.6-1.1 1.6 1.6-1.1 1.6.6 1.3 1.9.4v2.2l-1.9.4-.6 1.3 1.1 1.6-1.6 1.6-1.6-1.1-1.3.6-.4 1.9H6.9l-.4-1.9-1.3-.6-1.6 1.1-1.6-1.6 1.1-1.6-.6-1.3-1.9-.4V6.9l1.9-.4.6-1.3-1.1-1.6 1.6-1.6 1.6 1.1 1.3-.6z" stroke-linejoin=round /></svg></button>'
        '<button id=matrixbtn class="tiny icon" title="Chat Matrix: every running session in one window" '
        'aria-label="Chat Matrix" aria-pressed=false>'
        '<svg viewBox="0 0 16 16" width=14 height=14 aria-hidden=true>'
        '<rect x=1.5 y=1 width=5.5 height=6.5 rx=1 /><rect x=9 y=1 width=5.5 height=6.5 rx=1 />'
        '<rect x=1.5 y=8.5 width=5.5 height=6.5 rx=1 /><rect x=9 y=8.5 width=5.5 height=6.5 rx=1 />'
        '</svg></button></span>'
        '<span><button id=new class=tiny>+ new</button> '
        '<button class=tiny data-view=history title="search and resume past sessions">history</button> '
        '<button class=tiny data-view=insights title="tool failures and notifications">insights</button>'
        '</span></h1>'
        '<div id=meter class=meters tabindex=0 role=button aria-expanded=false '
        'title="Click for spend and the model mix"></div>'
        '<div id=tools><input id=filter placeholder="filter sessions… (/)" autocomplete=off '
        'title="keys: j/k next/previous session · n or alt+N next one waiting for you · alt+1-9 nth session · ctrl+K jump to a session · / filter">'
        '<select id=group class=tiny title="group sessions">'
        '<option value=state>by state</option><option value=folder>by folder</option>'
        '<option value=none>no groups</option></select></div>'
        '<div id=picker hidden><input id=find placeholder="project…" autocomplete=off>'
        '<div id=projects></div></div>'
        '<div id=sessions></div></div>'
        '<div id=main><div id=decide hidden></div>'
        '<div id=head><div id=headrow><div id=headtext><div id=title>loading…</div><div id=sub></div></div>'
        '<div id=headctl>'
        '<select id=tabpick class=tiny title="what the session pane shows">'
        '<option value=log selected>Conversation</option><option value=changes>Changes</option>'
        '<option value=activity>Activity</option><option value=worktrees>Worktrees</option></select>'
        '<details id=launch class=menu><summary title="start another session from this one">'
        'New / Fork</summary><div class=pop>'
        '<button data-act=new title="a fresh claude session in this project, in a new tab, selected here once it starts">New</button>'
        '<button data-act=new-close title="a fresh session in this project, selected here; this one is then closed (its conversation stays resumable)">New+close</button>'
        '<button data-act=fork-window title="a copy of this conversation in a new tab; the original is untouched">Fork</button>'
        '<button data-act=fork-background title="a copy, continued in the background with the prompt box as its message">Fork in bg</button>'
        '</div></details>'
        '<details id=viewmenu class=menu><summary title="resume command, current prompt, and how each kind of text shows">'
        'View</summary><div class=pop>'
        '<button id=copybtn data-act=copy>Copy resume cmd</button>'
        '<button id=curbtn title="the prompt this session is working on">Show Current Prompt</button>'
        '<div class=sep></div><div class=cap>Display shapes</div>'
        '<div id=shape title="how each kind of text shows in the chat"></div>'
        '</div></details>'
        '<div id=findbar><input id=cfind placeholder="find in chat (ctrl+F)" autocomplete=off '
        'title="Enter: earlier match · shift+Enter: later · Esc: clear">'
        '<select id=cfilter class=tiny title="what to search"><option value="">all text</option>'
        '<option value=tool>tool calls</option><option value=fail>failed results</option></select>'
        '<span id=cfcount></span><button class=ghost id=cfprev title="earlier match (Enter)">&#9650;</button>'
        '<button class=ghost id=cfnext title="later match (shift+Enter)">&#9660;</button></div>'
        '<div id=acts></div>'
        '<div id=jump><button class=ghost id=jprev title="previous prompt (alt+Up)">&#9650;</button>'
        '<button class=ghost id=jnext title="next prompt (alt+Down)">&#9660;</button></div>'
        '</div></div></div>'
        '<div id=sessionpane><div id=current hidden><div class=hd id=curhd>'
        '<span id=curcap>current prompt</span><span id=curfold>&#9662;</span></div>'
        '<div class=bd id=curbody></div></div><div id=log></div>'
        '<div id=procs hidden><div id=procgrip title="drag to resize"></div>'
        '<div id=proctabs role=tablist></div><pre id=proctext></pre></div>'
        '<div id=changes class=panel hidden></div><div id=activity class=panel hidden></div>'
        '<div id=worktrees class=panel hidden></div></div>'
        '<div id=historypane class=panel hidden>'
        '<input id=hq placeholder="search past sessions - words in any order" autocomplete=off>'
        '<div id=hits class=hits></div></div>'
        '<div id=insightpane class=panel hidden>'
        '<div class=card><h3>Notifications</h3><div id=notif></div></div>'
        '<div class=card><h3>Estimated cost <select id=cdays class=tiny>'
        '<option value=7>last 7 days</option><option value=30 selected>last 30 days</option>'
        '<option value=90>last 90 days</option></select></h3><div id=costs></div></div>'
        '<div class=card><h3>Tool failures <select id=days class=tiny>'
        '<option value=1>last day</option><option value=7 selected>last 7 days</option>'
        '<option value=30>last 30 days</option></select></h3><div id=errs></div></div></div>'
        '<div id=ask><textarea id=prompt placeholder="message this session…"></textarea>'
        '<div id=bar><span class=split><button id=send title="Enter">Send</button>'
        '<details id=sendmore class=menu><summary title="more ways to send" aria-label="more ways to send"></summary>'
        '<div class=pop><button id=sendtyped title="type it at the session&#39;s prompt as one line, as if you typed it there - '
        'it counts as you (turn on in Settings)">Send As Typed</button></div></details></span>'
        '<button id=queue class=ghost title="hold until its current turn ends (alt+Enter)">Queue</button>'
        '<button id=bg class=ghost title="a new background session (ctrl+Enter)">Background</button>'
        '<button id=mic class=ghost title="dictate (F9)">&#127908; Dictate</button>'
        '<span id=say></span></div><div id=heavy></div><div id=status hidden></div><div id=queued></div></div>'
        "</div></div>"
        '<div id=quickbox hidden><div class=box><input id=qq placeholder="quick phrase…" autocomplete=off>'
        '<div id=qlist></div><div class=keys>&#8593;&#8595; choose · Enter or 1-9 put it in the box · Esc back · '
        'phrases and this shortcut are in Settings</div></div></div>'
        '<div id=jumpbox hidden><div class=box><input id=jq placeholder="jump to a session…" autocomplete=off>'
        '<div id=jlist></div><div class=keys>&#8593;&#8595; choose · Enter open · Esc close · '
        'elsewhere: j/k next/previous · n next waiting for you · alt+1-9</div></div></div>'
        "<script>%s</script></body></html>" % (CSS, SCRIPT)
    ))


# -- the chat matrix ---------------------------------------------------------
# Every running session at once, in its own window: a tile each, holding
# Claude's replies and the one step it is on. The dispatcher does the cutting
# down (see matrix.py); this page only lays the tiles out and fills them.

MATRIX_CSS = """
body{overflow:hidden}
#mx{position:fixed;inset:0}
#grid{position:absolute;display:grid}
.tile{background:var(--panel);border:1px solid var(--line);border-radius:8px;
  display:flex;flex-direction:column;min-width:0;min-height:0;overflow:hidden}
.tile[data-k]{cursor:pointer;user-select:none}
/* The ring goes outside the border: an inset one is painted under the tile's
   own contents, so the header hid it at the top and only some sides showed. */
.tile.attn{border-color:var(--warn);box-shadow:0 0 0 1px var(--warn)}
.tile.fail{border-color:var(--fail);box-shadow:0 0 0 1px var(--fail)}
.th{display:flex;align-items:center;gap:6px;padding:6px 9px;border-bottom:1px solid var(--line);
  background:var(--side);font-size:13px;flex:none;min-width:0}
.th .dot{margin-right:0;flex:none}
.th .nm{font-weight:700;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
.th .meta{margin-left:auto;color:var(--muted);font-size:12px;white-space:nowrap;flex:none}
.tile .why{padding:3px 9px;font-size:12px;color:var(--warn);background:var(--warnbg);flex:none;
  white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
/* No scrolling: the newest reply sits at the bottom and older ones run off
   the top, faded. The main window is where a conversation gets read. */
.tb{flex:1;overflow:hidden;padding:4px 9px 8px;font-size:13px;line-height:1.45;min-height:0;
  display:flex;flex-direction:column;justify-content:flex-end;
  -webkit-mask-image:linear-gradient(to bottom,transparent 0,#000 28px);
  mask-image:linear-gradient(to bottom,transparent 0,#000 28px)}
.tb > *{flex:none}
.tb .rp{padding:5px 0;overflow-wrap:anywhere}
.tb .rp.newturn{border-top:1px dashed var(--line);margin-top:4px;padding-top:8px}
.tb .rp .txt{white-space:pre-wrap}
.tb pre.code{background:var(--code);padding:5px 7px;border-radius:5px;font-size:12px;line-height:1.4;
  font-family:var(--mono);overflow:hidden;margin:4px 0;white-space:pre}
.tb code{font-family:var(--mono);font-size:12px;background:var(--code);padding:0 3px;border-radius:3px}
.tb .md ul,.tb .md ol{margin:2px 0;padding-left:18px}
.tb .h{font-weight:700;margin:4px 0 2px}
.tb blockquote{margin:3px 0;padding-left:8px;border-left:3px solid var(--line);color:var(--muted)}
.tb table{border-collapse:collapse;font-size:12px}
.tb td,.tb th{border:1px solid var(--line);padding:1px 5px}
.tb .none{color:var(--muted);font-style:italic;padding:8px 0}
.cur{display:flex;align-items:center;gap:6px;margin-top:6px;padding:4px 7px;border-radius:5px;
  background:var(--sel);font-size:12px;min-width:0}
.cur b{flex:none}
.cur .what{font-family:var(--mono);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
.cur.thought{background:transparent;border:1px dashed var(--line);color:var(--muted);font-style:italic}
.cur.thought .what{font-family:inherit}
.cur.wait .what{font-family:inherit}
.cur .more{flex:none;color:var(--muted)}
.spin{flex:none;width:8px;height:8px;border-radius:50%;background:var(--busy);
  animation:pulse 1.2s ease-in-out infinite}
/* The state dot in a working tile's header pulses with its step, so work
   shows at a glance even with the step line run off or not yet there. */
.th .dot.d-busy{animation:pulse 1.2s ease-in-out infinite}
@keyframes pulse{50%{opacity:.25}}
@media (prefers-reduced-motion: reduce){.spin,.th .dot.d-busy{animation:none}}
.stat{padding:10px 12px;overflow:hidden;font-size:13px}
.stat h2{font-size:12px;text-transform:uppercase;letter-spacing:.07em;color:var(--muted);margin:0 0 6px}
.stat .big{font-size:26px;font-weight:700;line-height:1.1}
.stat .big.tok{font-size:20px}
.stat .sub{color:var(--muted);font-size:12px;margin-bottom:8px}
.stat table{width:100%;border-collapse:collapse;font-size:12px}
.stat td{padding:2px 0;vertical-align:top}
.stat td.n{text-align:right;white-space:nowrap;padding-left:8px}
.stat .meters.top{margin-bottom:12px}
.stat .sec{margin-top:10px}
#gone{position:fixed;left:50%;top:10px;transform:translateX(-50%);background:var(--warnbg);
  color:var(--warn);border:1px solid var(--warn);border-radius:5px;padding:3px 10px;font-size:12px}
"""

MATRIX_SCRIPT = SHARED + r"""
const $ = s => document.querySelector(s);

// Tiles are 1 wide by 1.5 high: a chat reads down, so a tile is a column.
const RATIO = 1.5, GAP = 8, EDGE = 10;

// The grid with the biggest tiles that fit: every column count is tried, each
// with as many rows as it needs, and the tile is as large as the tighter of
// width and height allows at that ratio. On a tie the grid with fewer empty
// cells wins. Empty cells only ever fall at the end of the last row, since a
// grid of `cols` columns never needs more than `cols - 1` of them.
function layout(n, W, H, gap = GAP, ratio = RATIO){
  n = Math.max(1, n);
  let best = null;
  for (let cols = 1; cols <= n; cols++){
    const rows = Math.ceil(n / cols);
    const w = Math.min((W - gap * (cols - 1)) / cols, (H - gap * (rows - 1)) / rows / ratio);
    if (!(w > 0)) continue;
    const spare = rows * cols - n;
    if (!best || w > best.w + 0.5 || (w > best.w - 0.5 && spare < best.spare))
      best = {cols, rows, w, h: w * ratio, spare};
  }
  return best || {cols: 1, rows: 1, w: 0, h: 0, spare: 0};
}

const money = v => typeof v === "number" ? "$" + v.toFixed(2) : "";
function count(v){
  if (!(v > 0)) return "0";
  if (v >= 1e9) return (v / 1e9).toFixed(1) + "B";
  if (v >= 1e6) return (v / 1e6).toFixed(1) + "M";
  if (v >= 1e3) return (v / 1e3).toFixed(1) + "k";
  return String(Math.round(v));
}

function project(cwd){
  const parts = String(cwd || "").split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || "";
}

// What a working session with no step in flight is waiting on: the same
// counts as the main window's tile (subagents / background shells / monitors
// / shell processes), in words. "" when it is not working or has none, so a
// session waiting on a long build is not mistaken for one stuck.
const WORK = [["agents", "subagent"], ["shells", "background shell"],
              ["monitors", "monitor"], ["processes", "process"]];
function waitingOn(t){
  if (t.current || stateOf(t)[0] !== "d-busy") return "";
  const w = t.work || {};
  return WORK.filter(([k]) => w[k] > 0)
    .map(([k, word]) => `${w[k]} ${word}${w[k] === 1 ? "" : word.endsWith("s") ? "es" : "s"}`).join(" · ");
}

// In a turn with nothing in flight and nothing it started to wait on: Claude
// is writing, or between one step and the next. Without a line for it the
// tile showed no sign of work at all. Not for a session idle between turns
// with a background task: that one is waiting, and says so.
function agentWorking(t){
  return !t.current && stateOf(t)[0] === "d-busy" && (!!RUNNING[t.status] || t.state === "working");
}

// Claude's replies, a rule between turns, and under them the step in flight.
function tileBody(t){
  let html = "", turn = null;
  for (const r of t.replies || []){
    const rule = turn !== null && r.turn !== turn;
    turn = r.turn;
    html += `<div class="rp${rule ? " newturn" : ""}">${renderText(r.text)}</div>`;
  }
  if (!html && !t.current)
    html = `<div class="none">${esc(t.error || "no replies yet")}</div>`;
  const c = t.current;
  if (c && c.kind === "tool")
    html += `<div class="cur" title="${esc(c.name + " " + (c.summary || ""))}"><span class="spin"></span>` +
      `<b>${esc(c.name)}</b><span class="what">${esc(c.summary || "")}</span>` +
      (c.more ? `<span class="more" title="more calls running">+${c.more}</span>` : "") + `</div>`;
  else if (c && c.kind === "thinking")
    html += `<div class="cur thought"><span class="spin"></span><span class="what">thinking${
      c.text ? " · " + esc(c.text) : ""}</span></div>`;
  const wait = waitingOn(t);
  if (wait)
    html += `<div class="cur wait" title="working, waiting on what it started"><span class="spin"></span>` +
      `<span class="what">waiting on ${esc(wait)}</span></div>`;
  else if (agentWorking(t))
    html += `<div class="cur wait" title="in a turn - writing or about to call a tool"><span class="spin"></span>` +
      `<span class="what">Agent working</span></div>`;
  return html;
}

function tileHead(t){
  const [dot, label] = stateOf(t);
  const meta = [money(t.cost_usd),
    typeof t.context_pct === "number" ? Math.round(t.context_pct) + "%" : ""].filter(Boolean).join(" · ");
  const where = project(t.cwd);
  return `<span class="dot ${dot}" title="${esc(label)}"></span>` +
    `<span class="nm" title="${esc(t.name || "")}${where ? " - " + esc(where) : ""}">${
      esc(t.name || t.sessionId || "")}</span>` +
    (meta ? `<span class="meta" title="the whole session's estimated cost · context used">${meta}</span>` : "");
}

function statusTile(sp){
  sp = sp || {tokens: {}, each: [], limits: {}};
  const tk = sp.tokens || {};
  // The sidebar's meters and model mix, open, at the top.
  const meters = meterHtml({limits: sp.limits, plan: sp.plan,
                            spend: sp.global ? {global: sp.global, current: {models: sp.models}} : null},
                           {lines: false});
  const rows = (sp.each || []).map(e => `<tr><td>${esc(e.name || "")}</td>` +
    `<td class="n">${count(e.tokens)}</td><td class="n">${money(e.cost_usd) || "-"}</td></tr>`).join("");
  const n = sp.sessions || 0;
  // Global: this machine's sends in the account's windows, open or not.
  const wins = ((sp.global || {}).windows || []).map(w => `<tr title="${esc(windowTitle(w) + "\n" + spendDetail(w))}">` +
    `<td>${esc(SPAN_TITLES[w.name] || w.name)}</td><td class="n">${count(w.total_tokens)}</td>` +
    `<td class="n">${money(w.cost_usd || 0)}</td></tr>`).join("");
  return `${meters ? `<div class="meters top">${meters}</div>` : ""}
    <h2>Current sessions</h2>
    <div class="big">${money(sp.cost_usd || 0)}</div>
    <div class="sub">${n} open session${n === 1 ? "" : "s"}, each over its whole life${
      sp.costed < n ? ` · cost known for ${sp.costed}` : ""}${sp.complete === false ? " · counting…" : ""}</div>
    <div class="big tok">${count(sp.total_tokens)} tokens</div>
    <div class="sub">in ${count(tk.input)} · out ${count(tk.output)} · cache read ${
      count(tk.cache_read)} · cache write ${count(tk.cache_write)}</div>
    ${wins ? `<div class="sec" title="${esc(SPEND_NOTE)}"><h2>Global</h2><table>${wins}</table></div>` : ""}
    ${rows ? `<div class="sec"><h2>By session</h2><table>${rows}</table></div>` : ""}`;
}

let data = {tiles: [], spend: null};

// Tiles are kept between polls and only refilled when what they show changes,
// so a tile someone has scrolled up in stays where they put it.
function draw(){
  const grid = $("#grid");
  const W = innerWidth - 2 * EDGE, H = innerHeight - 2 * EDGE;
  const tiles = data.tiles || [];
  // The ratio picks the grid; then the tiles grow to fill the window, giving
  // up the ratio rather than leaving bands of empty space around it.
  const L = layout(tiles.length, W, H);
  const cw = Math.max(0, (W - GAP * (L.cols - 1)) / L.cols);
  const rh = Math.max(0, (H - GAP * (L.rows - 1)) / L.rows);
  Object.assign(grid.style, {
    gridTemplateColumns: `repeat(${L.cols}, ${cw}px)`, gridAutoRows: `${rh}px`, gap: GAP + "px",
    left: EDGE + "px", top: EDGE + "px"});

  const keep = new Map([...grid.querySelectorAll(".tile[data-k]")].map(el => [el.dataset.k, el]));
  const order = [];
  for (const t of tiles){
    const k = t.key || t.sessionId;
    let el = keep.get(k);
    keep.delete(k);
    if (!el){
      el = document.createElement("div");
      el.className = "tile";
      el.dataset.k = k;
      el.dataset.sid = t.sessionId || k;
      el.title = "double-click to show this session in the main window";
      el.innerHTML = `<div class="th"></div><div class="why" hidden></div><div class="tb"></div>`;
    }
    const [dot, label] = stateOf(t);
    el.classList.toggle("attn", dot === "d-attn");
    el.classList.toggle("fail", dot === "d-fail");
    const head = tileHead(t);
    if (el.dataset.head !== head){ el.querySelector(".th").innerHTML = head; el.dataset.head = head; }
    const why = el.querySelector(".why");
    why.hidden = !t.attention;
    why.textContent = t.attention ? label : "";
    const body = tileBody(t);
    if (el.dataset.body !== body){ el.querySelector(".tb").innerHTML = body; el.dataset.body = body; }
    order.push(el);
  }
  keep.forEach(el => el.remove());

  // The empty cells at the end become one status tile; none when the grid is
  // full. With no sessions at all, the status tile is the whole matrix.
  const span = tiles.length ? L.spare : 1;
  let st = grid.querySelector(".tile.stat");
  if (span){
    if (!st){ st = document.createElement("div"); st.className = "tile stat"; }
    st.style.gridColumn = `span ${span}`;
    const html = statusTile(data.spend);
    if (st.dataset.sig !== html){ st.innerHTML = html; st.dataset.sig = html; }
    order.push(st);
  } else if (st) st.remove();
  order.forEach((el, i) => { if (grid.children[i] !== el) grid.insertBefore(el, grid.children[i] || null); });
}

// The main window's button lights while this window is up, and closes it
// through here - which still works after the main window has reloaded and
// lost the handle it opened this with.
const channel = "BroadcastChannel" in window ? new BroadcastChannel("imperatorium-matrix") : null;
let answered = null;
if (channel){
  channel.onmessage = e => {
    if (e.data === "close") window.close();
    else if (e.data && e.data.selected) answered = e.data.selected;
  };
  const beat = () => channel.postMessage("open");
  beat();
  setInterval(beat, 1000);
  addEventListener("pagehide", () => channel.postMessage("closed"));
}

async function get(url){
  const r = await fetch(url, {cache: "no-store"});
  if (!r.ok) throw new Error(r.status);
  return r.json();
}

async function poll(){
  try {
    data = await get("/api/matrix");
    $("#gone").hidden = true;
    draw();
  } catch (e) { $("#gone").hidden = false; }
  setTimeout(poll, 1500);
}

// Double click: show that session in the main window. Raising a window
// needs this click, so the window that opened this one is raised from here;
// the main window selects the session when asked on the channel. If no main
// window answers, a new one is opened on that session.
function openInMain(sid){
  answered = null;
  try { if (window.opener && !window.opener.closed) window.opener.focus(); } catch (e) {}
  if (channel) channel.postMessage({select: sid});
  setTimeout(() => {
    if (answered !== sid) window.open("/?session=" + encodeURIComponent(sid), "imperatorium-main");
  }, channel ? 400 : 0);
}

$("#grid").addEventListener("dblclick", e => {
  const el = e.target.closest(".tile[data-k]");
  if (!el) return;
  getSelection().removeAllRanges();
  openInMain(el.dataset.sid);
});

addEventListener("resize", draw);
poll();
"""


SETTINGS_CSS = """
body{overflow:auto}
#set{max-width:720px;margin:0 auto;padding:16px 16px 96px}
#set h1{font-size:20px;margin:0 0 2px}
#set .path{color:var(--muted);font-size:12px;margin-bottom:12px;word-break:break-all}
#set section{border-top:1px solid var(--line);padding:12px 0 6px}
#set h2{font-size:12px;text-transform:uppercase;letter-spacing:.07em;color:var(--muted);margin:0 0 8px}
#set .f{display:grid;grid-template-columns:200px 1fr;gap:4px 14px;margin:0 0 12px;align-items:start}
#set .f label{font-weight:600;padding-top:4px}
#set .f .ctl{display:flex;flex-direction:column;gap:3px;min-width:0}
#set input[type=text],#set input[type=number],#set select,#set textarea{background:var(--panel);
  color:var(--ink);border:1px solid var(--line);border-radius:5px;padding:5px 7px;
  font:13px var(--ui);max-width:100%}
#set textarea{font-family:var(--mono);min-height:58px;resize:vertical}
#set input[type=checkbox]{width:16px;height:16px;margin:5px 0 0}
#set .help{color:var(--muted);font-size:12px}
#set .when{font-size:12px;color:var(--muted)}
#set .when.live{color:var(--ok)}
#set .f.dirty label::after{content:" \\2022";color:var(--busy)}
#set .f.bad input,#set .f.bad textarea,#set .f.bad select{border-color:var(--fail)}
#set .err{color:var(--fail);font-size:12px}
#set .hosts{margin:0;padding-left:18px;font-size:13px}
#set table.keys{border-collapse:collapse;font-size:13px;width:100%}
#set table.keys th{text-align:left;font-size:12px;color:var(--muted);font-weight:600;padding:10px 0 3px}
#set table.keys td{padding:3px 14px 3px 0;vertical-align:top}
#set table.keys td:first-child{width:40%;line-height:1.9}
#set kbd{font:12px var(--mono);border:1px solid var(--line);border-bottom-width:2px;border-radius:4px;
  padding:0 5px;background:var(--panel);white-space:nowrap}
#bar{position:fixed;left:0;right:0;bottom:0;background:var(--side);border-top:1px solid var(--line);
  padding:10px 16px;display:flex;gap:10px;align-items:center;flex-wrap:wrap}
#bar .msg{flex:1;min-width:200px;font-size:13px}
#bar .msg.bad{color:var(--fail)}
#bar button{font:13px var(--ui);padding:5px 14px;border-radius:5px;border:1px solid var(--line);
  background:var(--panel);color:var(--ink);cursor:pointer}
#bar button.go{background:var(--busy);border-color:var(--busy);color:var(--onaccent)}
#bar button:disabled{opacity:.5;cursor:default}
#wait{border:1px solid var(--warn);background:var(--warnbg);border-radius:6px;padding:8px 10px;
  margin:0 0 12px;font-size:13px}
#wait code{font-family:var(--mono)}
@media (max-width:600px){#set .f{grid-template-columns:1fr}}
"""

SETTINGS_SCRIPT = SHARED + r"""
const $ = s => document.querySelector(s);
let form = null, original = {};
const LIST = {paths: 1, hotkeys: 1, names: 1, phrases: 1};
const WHEN = {live: "applies at once", dispatcher: "after a dispatcher restart",
              hotkey: "after the palette restarts", streamdeck: "after the Stream Deck restarts"};

// What a field holds, as the form shows it, so "changed" is a string compare.
function shown(f, v){
  if (f.type === "bool") return !!v;
  if (LIST[f.type]) return (v || []).join("\n");
  if (f.type === "numbers") return (v || []).join(", ");
  return v == null ? "" : String(v);
}
function control(f){
  const id = "f-" + f.key.replace(/\W/g, "_"), v = shown(f, f.value);
  if (f.type === "bool") return `<input type=checkbox id="${id}"${v ? " checked" : ""}>`;
  if (f.type === "choice")
    return `<select id="${id}"${f.locked ? " disabled" : ""}>` + f.choices.map(c =>
      `<option value="${esc(c)}"${c === v ? " selected" : ""}>${esc(c || "(your own setting)")}</option>`).join("") +
      `</select>` + (f.locked ? `<span class="help">Set in config.json; change it there.</span>` : "");
  if (LIST[f.type]) return `<textarea id="${id}" rows=${Math.max(2, Math.min(8, (f.value || []).length + 1))}` +
    ` spellcheck=false>${esc(v)}</textarea>`;
  if (f.type === "key")
    return `<input type=text id="${id}" value="${esc(v)}" spellcheck=false data-record=1 ` +
      `placeholder="press the keys">`;
  if (f.type === "int" || f.type === "optint")
    return `<input type=number id="${id}" min="${f.min}" max="${f.max}" step=1 value="${esc(v)}">`;
  return `<input type=text id="${id}" value="${esc(v)}" spellcheck=false>`;
}
function read(f){
  const el = document.getElementById("f-" + f.key.replace(/\W/g, "_"));
  return f.type === "bool" ? el.checked : el.value;
}
// The value as the server wants it.
function wire(f, v){
  if (LIST[f.type]) return v.split("\n").map(x => x.trim()).filter(Boolean);
  if (f.type === "int") return v === "" ? null : Number(v);
  if (f.type === "optint") return v === "" ? null : Number(v);
  return v;
}

function draw(d){
  form = d; original = {};
  const by = {};
  for (const f of d.fields) { (by[f.section] = by[f.section] || []).push(f); original[f.key] = shown(f, f.value); }
  const wait = (d.waiting || []).map(p => `<div>${esc(WHEN[p] || p)}: <code>${esc(d.restart[p] || "")}</code></div>`).join("");
  $("#set").innerHTML = `<h1>Settings</h1><div class="path">${esc(d.path || "")}</div>` +
    (wait ? `<div id="wait"><b>Saved, waiting for a restart</b>${wait}</div>` : "") +
    d.sections.map(sec => `<section><h2>${esc(sec.title)}</h2>` + (by[sec.id] || []).map(f => {
      const id = "f-" + f.key.replace(/\W/g, "_");
      return `<div class="f" data-key="${esc(f.key)}"><label for="${id}">${esc(f.label)}</label><div class="ctl">` +
        control(f) + (f.help ? `<span class="help">${esc(f.help)}</span>` : "") +
        `<span class="when ${f.applies === "live" ? "live" : ""}">${esc(WHEN[f.applies] || "")}</span>` +
        `<span class="err" hidden></span></div></div>`;
    }).join("") + (sec.id === "remote" ? hostsHtml(d.hosts) : "") + `</section>`).join("") +
    shortcutsHtml(d);
  $("#set").oninput = $("#set").onchange = mark;
  // A key field takes the combination pressed in it rather than typed text.
  for (const el of document.querySelectorAll("#set [data-record]")) el.onkeydown = e => {
    if (e.key === "Tab" || (!e.ctrlKey && !e.altKey && !e.metaKey && e.key !== "Backspace")) return;
    e.preventDefault();
    if (e.key === "Backspace" && !e.ctrlKey && !e.altKey && !e.metaKey) { el.value = ""; mark(); return; }
    const c = comboOf(e);
    if (c) { el.value = c; mark(); }
  };
  mark();
}
// Read-only: every key the window answers to (ui.SHORTCUTS). A key that is
// itself a setting shows that setting's saved value.
function shortcutsHtml(d){
  const list = d.shortcuts || [];
  if (!list.length) return "";
  const val = k => { const f = d.fields.find(x => x.key === k); return f ? f.value : null; };
  const keys = k => {
    const v = val(k);
    if (v == null) return esc(k);
    return (Array.isArray(v) ? v : [v]).map(x => `<kbd>${esc(prettyKey(x))}</kbd>`).join(" or ");
  };
  // "Up / Down, Enter" -> each key boxed, the separators left as text.
  const SEP = /^(, | \/ | - )$/;
  const caps = text => text.split(/(, | \/ | - )/)
    .map(p => SEP.test(p) ? esc(p) : `<kbd>${esc(p)}</kbd>`).join("");
  let where = null, rows = "";
  for (const s of list) {
    if (s.where !== where) { where = s.where; rows += `<tr><th colspan=2>${esc(where)}</th></tr>`; }
    rows += `<tr><td>${/^[a-z]+\.[a-z_]+$/.test(s.keys) ? keys(s.keys) : caps(s.keys)}</td>` +
      `<td>${esc(s.what)}</td></tr>`;
  }
  return `<section id="keys"><h2>Keyboard shortcuts</h2><table class="keys">${rows}</table></section>`;
}
function hostsHtml(hosts){
  if (!hosts || !hosts.length) return `<div class="help">No remote hosts.</div>`;
  return `<div class="f"><label>Hosts</label><ul class="hosts">` + hosts.map(h =>
    `<li>${esc(h.label || h.name)}${h.label && h.label !== h.name ? ` (${esc(h.name)})` : ""}${
      h.tunnel ? " · tunnel" : ""}</li>`).join("") + `</ul></div>`;
}
function changes(){
  const out = {};
  for (const f of form.fields) {
    if (f.locked) continue;
    const v = read(f);
    if ((f.type === "bool" ? v : String(v)) !== original[f.key]) out[f.key] = wire(f, v);
  }
  return out;
}
function mark(){
  const ch = changes();
  for (const el of document.querySelectorAll("#set .f[data-key]")) el.classList.toggle("dirty", el.dataset.key in ch);
  const n = Object.keys(ch).length;
  $("#save").disabled = $("#revert").disabled = !n;
  if (n) say(n + " unsaved change" + (n === 1 ? "" : "s"));
}
function say(text, bad){ $("#msg").textContent = text; $("#msg").classList.toggle("bad", !!bad); }

async function load(){
  try {
    const d = await (await fetch("/api/settings", {cache: "no-store"})).json();
    if (!d.ok) { say(d.error || "settings unavailable", true); return; }
    draw(d); say("");
  } catch (e) { say("dispatcher not answering", true); }
}
async function save(){
  const ch = changes();
  if (!Object.keys(ch).length) return;
  $("#save").disabled = true; say("saving…");
  let d;
  try {
    d = await (await fetch("/api/settings", {method: "POST", cache: "no-store",
      headers: {"Content-Type": "application/json", "X-Imperatorium-Window":
        (document.querySelector("meta[name=imp-window]") || {}).content || ""},
      body: JSON.stringify({changes: ch})})).json();
  } catch (e) { say("dispatcher not answering", true); $("#save").disabled = false; return; }
  for (const el of document.querySelectorAll("#set .f")) {
    el.classList.remove("bad");
    const e = el.querySelector(".err"); if (e) { e.hidden = true; e.textContent = ""; }
  }
  if (!d.ok) {
    for (const [k, m] of Object.entries(d.errors || {})) {
      const el = document.querySelector(`#set .f[data-key="${CSS.escape(k)}"]`);
      if (el) { el.classList.add("bad"); const e = el.querySelector(".err"); e.hidden = false; e.textContent = m; }
    }
    say(d.error || "not saved", true); $("#save").disabled = false; return;
  }
  draw(d);
  say("Saved." + ((d.restart || []).length ? " Some changes need a restart (see the top)." : " Applied."));
}
$("#save").onclick = save;
$("#revert").onclick = () => { draw(form); say("changes discarded"); };
addEventListener("keydown", e => { if ((e.ctrlKey || e.metaKey) && e.key === "s") { e.preventDefault(); save(); } });
addEventListener("beforeunload", e => { if (form && Object.keys(changes()).length) { e.preventDefault(); e.returnValue = ""; } });
load();
"""


def render_settings(token=""):
    return _with_meta(token, (
        "<!doctype html><html lang=en><head><meta charset=utf-8>"
        '<meta name=viewport content="width=device-width,initial-scale=1">'
        "<title>Imperatorium Settings</title><style>%s%s</style></head><body>"
        "<main id=set><h1>Settings</h1></main>"
        '<div id=bar><span id=msg class=msg role=status></span>'
        '<button id=revert disabled>Revert</button><button id=save class=go disabled>Save</button></div>'
        "<script>%s</script></body></html>" % (CSS, SETTINGS_CSS, SETTINGS_SCRIPT)
    ))


def render_matrix():
    return (
        "<!doctype html><html lang=en><head><meta charset=utf-8>"
        '<meta name=viewport content="width=device-width,initial-scale=1">'
        "<title>Chat Matrix</title><style>%s%s</style></head><body>"
        "<div id=mx><div id=grid></div></div>"
        "<div id=gone hidden>dispatcher not answering - retrying</div>"
        "<script>%s</script></body></html>" % (CSS, MATRIX_CSS, MATRIX_SCRIPT)
    )
