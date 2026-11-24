# UI names

One name for every part of the window, so a change, a bug or a screenshot
can be talked about without describing where on screen it is. Each name is
followed by the DOM id or class it maps to, so a name can be grepped straight
to `imperatorium/ui.py` (`render_shell` for the main window, `render_matrix` for
the Chat Matrix; everything else is filled in by the script).

## Convention

- Names are Title Case and nest with `›`: Window › Region › Component › Part,
  e.g. *Sidebar › Session Tile › Meta Line*.
- A **Window** is a browser window. A **Region** is a fixed part of the
  layout. A **Pane** is one of several views that take turns in the same
  area. A **Panel**, **Card** or **Bar** is a block inside a region. A
  **Badge**, **Button** or **Line** is a small part.
- In conversation use the shortest name that is unique ("the Meta Line",
  "the Find Window"); give the path only when it is ambiguous.
- A new part gets a name here in the same change that adds it.

## Main window (Imperatorium)

**1. Sidebar** `#side`
- **Sidebar Header** `#side h1` (no title text: the list speaks for itself)
  - Settings Button `#setbtn` (gear; opens the Settings window)
  - Matrix Button `#matrixbtn`
  - **Sidebar Menu** `#sidemenu` (☰; 2026-11-16, in place of three buttons):
    - New Session Item `#new` (opens the New Session Dialog)
    - History Item `[data-view=history]` (opens the History Pop-up)
    - Insights Item `[data-view=insights]` (opens the Insights Pop-up)
    - Project Changes Item `[data-view=project]` (opens the Project Changes
      Pop-up; the View Menu has one too)
- **Usage Meter** `#meter.meters`: Limit Bars (`.row`, in one grid `.rows` so
  every bar is the same length), Reset Tick, Percentage (`.pct`), Meter Tip
  (`.tip`, on hover: the window's name, use, reset time, time elapsed),
  Waiting Row (`.row.waiting`: the window has reset and no session has
  reported the new one yet; 0%, no tick),
  Meter Caret (`#meter::after`). Shows only the bars until clicked; a click
  opens the **Meter Details** (everything marked `.extra`, the meter gets
  `.open`) and another folds them, remembered per browser
  - Pace Line (`.pace`), Plan Label (`.plan`)
  - **Spend Lines** `.spend`: Global Line (this machine's spend in the
    account's windows) and Open Line (the open sessions, each over its whole
    life); detail in each line's title
  - **Model Mix** (`.cap`, `.row.mix`, `.legend`): a stacked bar per global
    window and for the open sessions, output tokens by model family in cost
    order (haiku, sonnet, opus, fable, other), colours from claude-lens's
    ironbow ramp (`--iron-1`…`--iron-4`, `--iron-other`); per-family detail
    in each row's Meter Tip
- **Sidebar Tools** `#tools`
  - Session Filter `#filter`
  - Group Selector `#group`
- **Session List** `#sessions`
  - Group Header `.g`
  - Archive Fold `#archfold`
  - **Family Header** `.fam`: the row for a folder with several sessions.
    Parts: Caret (`.car`), Family Name, New Count (`.newn`), Waiting-Hidden
    Tag (`.nowait`), Dot Row (`.dots`)
  - Family Children `.kids`
  - **Session Tile** `.s`
    - Tile Actions `.acts`: Pin, Close, Unarchive, Dismiss (archived, nothing running);
      in the open tile's top right corner (end of its Name Line), on no closed tile
    - Name Line `.n`: App Icon (or VS Code Icon), Worktree Icon, Pin Star, New Badge
    - Window Title Line `.w`
    - Doing Line `.d`
    - Meta Line `.m`: State Dot, State Label, Age, Host, PR Badge,
      Context Badge, Queued Badge, Cold Badge
    - **MCP Line** `.mcpl` (open tile only): MCP Mark `.mk` and the server
      names `.ml`, aligned with the Meta Line in its own colour (`--mcp`),
      above the Task List so it is not read as a task
    - **Task List** `.tk`: Tasks Header `.tkh`, Task Rows `.tki` (Kind Mark
      `.k`, plain text; Fresh-Output Dot `.live`, pulses)

**2. Main Area** `#main`
- **Decision Bar** `#decide`: Decision Rows (`.dq`) with Allow / Deny;
  a parked AskUserQuestion is a Question Card (`.dqq`) instead - one
  Question (`.qq`) per question with Option buttons (`.qo`, numbered
  `.qn`) and an Other box (`.qother`), then Submit answer / Answer in
  terminal / Decline (`.qa`) and Key Hints (`.qkeys`). With several
  questions one shows at a time: Question Tabs (`.qtabs`: ←/→ `.qnav`, a
  Question Tab `.qtab` each, the Submit Tab `.qsubmit`) and, on the Submit
  Tab, the Answer Review (`.qrev`)
- **Session Header** `#head`: one row, the title on the left and the
  Header Controls on the right
  - Session Title `#title`
  - Session Subtitle `#sub`: Branch Badge (`.hbr`), Worktree Tag (`.hwt`,
    or "main checkout"), then the folder (not the window title, which the
    tile shows)
  - **Header Controls** `#headctl`, left to right:
    - Tab Picker `#tabpick`: Conversation, Changes, Activity, Worktrees
    - **View Menu** `#viewmenu`: Show Window (`#showbtn`, for a running
      session on this machine), Copy Resume Button (`#copybtn`), Open Folder
      in Editor (`#editbtn`), Merge Worktree Back (`#mergebtn`, for a
      session in a worktree under `.claude/worktrees`), Current
      Prompt Button (`#curbtn`), Display Shapes (`#shape`) with Group Tool
      Calls (`[data-group]`, off by default) under the Tool results row,
      and under them Display Times (`[data-times]`, off by default: each
      block of the chat shows when it was written, as a Block Time `.bt`).
      Its button is lit while any kind of text is not shown in full or tool
      calls are grouped
    - Find `#findbar` (below): the Find Button, shown on the Conversation tab only
    - Session Actions `#acts`: the ones that stay buttons - Resume,
      Resume in bg. Between the two pairs of arrows, to keep
      them apart
    - Prompt Jump `#jump` (below), shown on the Conversation tab only
- **Session Pane** `#sessionpane`
  - **Current Prompt Card** `#current`
    - Card Header `#curhd`
    - Card Body `#curbody`
  - **Conversation Log** `#log`
    - Turn `.turn`, headed by its Speaker Line `.who`
    - Prompt Band: a turn of yours (`.turn.you-turn`) under
      `html.hl-prompts` (Settings > Highlighting > Your prompts), in
      `--hl-prompt` (Settings > Highlight colours > Your prompts)
    - Sent Prompt `.turn.pend` (`.sending`, `.sent`, `.failed`): a prompt
      sent from the window, at the end of the log until the conversation
      has it, with its Sent Note `.pnote` and a dismiss button
      (`[data-unpend]`) once it has an answer
    - Thinking Block `.think`
    - Tool Line `.tool`
    - Collapsed Block `details.cmp`
    - Tool Group `details.tgroup` (View > Group Tool Calls): a run of tool
      calls and results, collapsed to one line (count, tools, failures);
      its blocks inside in `.gb`
    - Compaction Marker `details.compact`
    - Path `.pth`: a click opens the Path Menu (below)
    - Question Record `.askq`: with several questions, Record Tabs
      (`.asktabs`) and one Record Page (`.askp`) at a time
    - Earlier Button `.earlier`
    - Via-Inbox Tag `.via`
    - Show Most Recent Button `.newest button`: floats at the bottom of
      the log while its end is out of view
  - **Task View** `#taskview`: a task opened from the tile's Task List, in
    place of the chat (Task Header `#tvhead`, Task Text `#tvtext`). The
    Process Drawer (`#procs`) that sat here was removed 2026-11-15
  - **Changes Panel** `#changes`: Review Box (`#review`, holding Line
    Comments `.rv`), Branch Line, File List, Diff View, Recent Commits
  - **Activity Panel** `#activity`: Totals Line, Event Rows (`.ev`)
  - **Worktrees Panel** `#worktrees`
    - Worktree Card `.card`
    - Worktree Diff `.wdiff`
- **History Pop-up** `#histbox` (over the window; Esc, ✕ `[data-popclose]` or a
  click outside returns to the chat), holding the History Pane `#historypane`
  - History Search `#hq`
  - History Filters `#histfilters`: Folder `#hfolder`, Sort `#hsort`, Active Within `#hdays`
  - History Folder Note `#histbias` (opened by /resume: the session's folder, with Show All Folders `#hall`)
  - History Results `#hits`, made of History Hits (`.hit`)
- **Insights Pop-up** `#insightbox`, likewise, holding the Insights Pane `#insightpane`
  - Cost Card `#costs`: Cost Chart (`svg.chart`) and Cost Tables
  - Tool Failures Card `#errs`
- **Project Changes Pop-up** `#projbox`, likewise: Project Picker `#pjpick`,
  Refresh `#pjrefresh`, and the Project Pane `#projpane`
  - Overlaps Card: one Overlap `.pjov` per file more than one line of work
    changes, opening to each line's diff of it (`.pjdiffs`)
  - Line Cards `.pjline` (edged `.meets` when they hold an overlap): one
    per line of work - the main checkout, each worktree, each branch ahead
    of its base without one - with Session Chips `.pjs`, the File List
    `.pjfiles` (a File Row `.pjf` opens its diff `.pjfd`; `.shared` when
    another line changes it too) and, for a background session's worktree,
    Merge / Push + open PR / Discard
- In the Header Controls (above), from what was the Chat Strip:
  - **Find** `#findbar`
    - **Find Button** `#findbtn`: opens and closes the Find Window (ctrl+F
      opens it too); lit while it is open
    - **Find Window** `#findpop`: floats under the button and leaves the
      chat usable; closing it (its close, Esc, or the button again) clears
      the find
      - Find Input `#cfind`
      - Find Filter `#cfilter`
      - Match Count `#cfcount`
      - Previous / Next Match `#cfprev` / `#cfnext`
      - Close Find `#cfclose`
  - **Prompt Jump** `#jump`: Previous / Next Prompt `#jprev` / `#jnext`
- **Prompt Dock** `#ask`
  - **Prompt Box** `#prompt`
  - **Send Bar** `#bar`
    - **Send Split** `.split`: Send `#send` and the Send Menu `#sendmore`
      (its ▾), which holds the New Session Options (`[data-spawn]`: Send to
      New Session, ... with Context, Send to New Background Session, ...
      with Context) and Send As Typed `#sendtyped`
    - Stop `#stop` (only while the session works; Esc in the Prompt Box
      does the same)
    - Queue `#queue`, Dictate `#mic`
    - Say Line `#say`
  - **Heavy Warning** `#heavy`
  - **Status Panel** `#status`
    - Status Line `.hd`: the line that stays when the panel is folded
    - Status Grid `.grid` of Status Cells (`.c`)
    - Run List `.run`
  - **Queue List** `#queued`: Queued Items (`.qi`)

**3. Jump Box** `#jumpbox`: the ctrl+K overlay
- Jump Input `#jq`
- Jump List `#jlist`, made of Jump Rows (`.ji`)
- Key Hints `.keys`

**4. Quick Phrase Box** `#quickbox`: the overlay the quick-phrase shortcut
opens from the Prompt Box
- Phrase Filter `#qq`
- Phrase List `#qlist`, made of Phrase Rows (`.ji`, numbered `.qn`)
- Key Hints `.keys`

**5. New Session Dialog** `#newbox`: from the Sidebar Menu
- Project Search `#find`
- Project List `#projects`, made of Project Rows (`.p`)
- Permission Mode `#nmode`, Model `#nmodel`, Effort `#neffort`
- Background Option `#nbg`: start it in the background, the Prompt Box as its task
- Remote Note `#nbnote`, Start `#nstart`, Cancel `#ncancel`, Key Hints `.keys`

**6. Path Menu** `#pathmenu`: a click on a Path in the chat - the path
(`.cap`), then Open, Open folder, Copy path (`[data-pact]`)

**7. New Session Check** `#spawnbox`: before another session is started in a
worktree, or when there is something to warn about - Check Text `#sbbody`
(with a Warning `.sbwarn`), Check Buttons `#sbbar` (`[data-sb]`: Create
worktree, Use the same folder, Copy changes and continue, Start without
context, Cancel), Check Footer `#sbfoot`

## Chat Matrix window

- **Matrix Grid** `#grid`
- **Matrix Tile** `.tile`
  - Tile Header `.th`: State Dot (pulses while working), Tile Name
    (`.nm`), Tile Meta (`.meta`)
  - Attention Line `.why`
  - Tile Body `.tb`
    - Reply `.rp`
    - Step Chip `.cur`, in three variants: tool, thought (`.thought`) and
      wait (`.wait`: "waiting on …" or "Agent working")
- **Stats Tile** `.tile.stat`
  - Tile Meters `.meters.top`: the Usage Meter's bars, pace and plan lines
    and Model Mix, always open (the spend lines are left out; the tile spells
    the spend out below)
  - Current Sessions: Spend Figure `.big`, token line
  - Global Table: this machine's spend in each of the account's windows
  - By-Session Table
- **Offline Banner** `#gone`

## Settings window (`/settings`)

- **Settings Form** `#set`: a section per part of `config.json`, each
  **Setting** `.f[data-key]` with its label, control, help, and **When Line**
  `.when` (`.live`: applies at once; otherwise which restart it waits on).
  Changed settings are marked `.dirty`, refused ones `.bad` with the reason
  in `.err`. The Notifications section also has the **Desktop Notification
  Switch** `#desk` (`#deskrow`; per browser, not in `config.json`, applies
  at once) and ntfy's last error when there is one.
- **Shortcut List** `#keys`: every keyboard shortcut (`ui.SHORTCUTS`), as a
  table under the form; keys that are settings show their saved value.
- **Restart Note** `#wait`: saved settings waiting for a restart, with the
  command.
- **Save Bar** `#bar`: status `#msg`, Revert `#revert`, Save `#save`
  (also ctrl+S).

## Names kept apart

- **Session Tile** (sidebar) and **Matrix Tile** (Chat Matrix): both are
  "tiles" in the code.
- **Session Actions** (`#acts`, in the Session Header) and **Tile Actions**
  (`.acts`, on each Session Tile).
- **Status Panel** (`#status`) and a session's state: the state is the
  **State Dot** / **State Label**, never "status".
- **Panels** and **Panes**: the tab bodies (Changes, Activity, Worktrees)
  are Panels. History and Insights are Panes, because each takes over the
  whole Main Area. All of them share the `.panel` class.
