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
  "the Find Bar"); give the path only when it is ambiguous.
- A new part gets a name here in the same change that adds it.

## Main window (Imperatorium)

**1. Sidebar** `#side`
- **Sidebar Header** `#side h1` (no title text: the list speaks for itself)
  - Settings Button `#setbtn` (gear; opens the Settings window)
  - Matrix Button `#matrixbtn`
  - New Button `#new`
  - History Button `[data-view=history]`
  - Insights Button `[data-view=insights]`
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
- **Project Picker** `#picker`
  - Project Search `#find`
  - Project List `#projects`
- **Session List** `#sessions`
  - Group Header `.g`
  - Archive Fold `#archfold`
  - **Family Header** `.fam`: the row for a folder with several sessions.
    Parts: Caret (`.car`), Family Name, New Count (`.newn`), Waiting-Hidden
    Tag (`.nowait`), Dot Row (`.dots`)
  - Family Children `.kids`
  - **Session Tile** `.s`
    - Tile Actions `.acts`: Pin, Close, Unarchive, Dismiss (archived, nothing running)
    - Name Line `.n`: App Icon (or VS Code Icon), Worktree Icon, Pin Star, New Badge
    - Window Title Line `.w`
    - Doing Line `.d`
    - Meta Line `.m`: State Dot, State Label, Age, Host, PR Badge,
      Context Badge, Queued Badge, Cold Badge

**2. Main Area** `#main`
- **Decision Bar** `#decide`: Decision Rows (`.dq`) with Allow / Deny
- **Session Header** `#head`: one row, the title on the left and the
  Header Controls on the right
  - Session Title `#title`
  - Session Subtitle `#sub`
  - **Header Controls** `#headctl`, left to right:
    - Tab Picker `#tabpick`: Conversation, Changes, Activity, Worktrees
    - **New / Fork Menu** `#launch`: New, New+close, Fork, Fork in bg
    - **View Menu** `#viewmenu`: Copy Resume Button (`#copybtn`), Current
      Prompt Button (`#curbtn`), Display Shapes (`#shape`). Its button is
      lit while any kind of text is not shown in full
    - Find Bar `#findbar` (below), shown on the Conversation tab only
    - Session Actions `#acts`: the ones that stay buttons - Resume,
      Resume in bg, Show Window. Between the two pairs of arrows, to keep
      them apart
    - Prompt Jump `#jump` (below), shown on the Conversation tab only
- **Session Pane** `#sessionpane`
  - **Current Prompt Card** `#current`
    - Card Header `#curhd`
    - Card Body `#curbody`
  - **Conversation Log** `#log`
    - Turn `.turn`, headed by its Speaker Line `.who`
    - Thinking Block `.think`
    - Tool Line `.tool`
    - Collapsed Block `details.cmp`
    - Compaction Marker `details.compact`
    - Earlier Button `.earlier`
    - Via-Inbox Tag `.via`
    - Show Most Recent Button `.newest button`: floats at the bottom of
      the log while its end is out of view
  - **Process Drawer** `#procs`
    - Drawer Grip `#procgrip`
    - Process Tabs `#proctabs`: one per running shell (`$`), Monitor
      (`mon`) and subagent (`agent`), led by an Auto Tab when there are
      several
    - Process Output `#proctext`
  - **Changes Panel** `#changes`: Review Box (`#review`, holding Line
    Comments `.rv`), Branch Line, File List, Diff View, Recent Commits
  - **Activity Panel** `#activity`: Totals Line, Event Rows (`.ev`)
  - **Worktrees Panel** `#worktrees`
    - Worktree Card `.card`
    - Worktree Diff `.wdiff`
- **History Pane** `#historypane`
  - History Search `#hq`
  - History Results `#hits`, made of History Hits (`.hit`)
- **Insights Pane** `#insightpane`
  - Cost Card `#costs`: Cost Chart (`svg.chart`) and Cost Tables
  - Tool Failures Card `#errs`
- In the Header Controls (above), from what was the Chat Strip:
  - **Find Bar** `#findbar`
    - Find Input `#cfind`
    - Find Filter `#cfilter`
    - Match Count `#cfcount`
    - Previous / Next Match `#cfprev` / `#cfnext`
  - **Prompt Jump** `#jump`: Previous / Next Prompt `#jprev` / `#jnext`
- **Prompt Dock** `#ask`
  - **Prompt Box** `#prompt`
  - **Send Bar** `#bar`
    - **Send Split** `.split`: Send `#send` and the Send Menu `#sendmore`
      (its ▾), which holds Send As Typed `#sendtyped`
    - Queue `#queue`, Background `#bg`, Dictate `#mic`
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
