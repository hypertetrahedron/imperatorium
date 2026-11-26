// The Imperatorium mod: the dispatcher's hand inside each Claude Code session.
//
// Every couple of seconds, and at once when a turn starts or ends or usage
// changes, it beats to the dispatcher (POST /api/mod/beat): busy or idle, the
// session's context, cost and rate limits. The reply carries the prompts
// waiting for this session, which it submits with `$.prompt.submit` - the
// call waits for the session to be idle - and the line it shows under the
// prompt. A prompt the person sent from the Imperatorium window comes marked
// `as_user` and is submitted as their own words; anything else is submitted
// framed under this mod's name.
//
// A reply can also ask it to stop the running turn (the window's Stop, or
// Esc there): it cancels the main loop's turn with `$.turn.abort`, as Esc at
// the terminal does, and says on its next beat what came of it.
//
// A reply can carry ops (0.4.0): a slash command the person sent from the
// window, run with `$.command.run` as if typed so its output goes back to
// the window; and reads of the session's context, usage, status, settings
// and /config rows, or one /config change, for the window to show. Each
// answer goes back on the next beat (`results`). It also lists the
// session's slash commands (`commands`) for the window's prompt box.
//
// It also answers `tool.check` from the dispatcher's permission policy
// (POST /api/mod/check), for the calls the session's rules and mode would put
// to the person. Anything unsure, slow or broken leaves the session's own
// decision standing.
//
// Every request carries the token the dispatcher keeps in its state folder
// (`mod-token`). With no dispatcher, the mod does nothing at all.
import type { EngineInterface, Register } from 'claude-code'

type Engine = EngineInterface

const MOD_VERSION = '0.5.0'
const BEAT_MS = 2000
// How often the session's command list is sent again: a dispatcher that
// restarted has forgotten it, and plugins come and go.
const COMMANDS_MS = 60000
// How long a permission check waits for the dispatcher before leaving the
// decision to the session: a tool call should never wait on it for long.
const CHECK_WAIT_MS = 1500
// A request the dispatcher has not answered by then is given up on: the
// engine's fetch has no timeout of its own, and a beat waiting forever would
// hold back every beat after it (`beating` stays set).
const POST_TIMEOUT_MS = 10_000
// Tools a hook can only tighten (a question to the person, a plan to
// approve): an allow from here would not dismiss their dialog.
const PERSON_TOOLS = new Set(['AskUserQuestion', 'ExitPlanMode'])

type Offered = { id: string; text: string; as_user: boolean }
type Ack = { id: string; outcome: 'accepted' | 'submitted' | 'dropped'; reason?: string }
type Op = { id: string; op: string; args?: Record<string, unknown> }
type OpResult = { id: string; ok: boolean; data?: unknown; error?: string }
type BeatReply = { ok?: boolean; deliver?: Offered[]; status?: string | null; stop?: string | null; ops?: Op[] }
// What came of a stop the dispatcher asked for, by its id.
type Stopped = { id: string; outcome: 'aborted' | 'idle' | 'failed'; reason?: string }
type CheckReply = { verdict?: string; reason?: string }

// The module's state. A reload starts it over: the dispatcher re-offers a
// prompt that was never accepted, and the next beat restores the rest.
let endpoint = 'http://127.0.0.1:8792'
// IMPERATORIUM_MOD_ENDPOINT was given: the dispatcher's own file does not
// override it.
let endpointFixed = false
let token: string | null = null
let sessionId = ''
let cwd = ''
let busy = false
let turnStarted: number | null = null
// The main loop's running turn, the id `$.turn.abort` takes.
let turnId: string | null = null
let stopped: Stopped[] = []
const stopsSeen = new Set<string>()
let interactive = false
let surfaces: readonly string[] = []
let version = ''
let usage: Record<string, unknown> | null = null
let usageDirty = false
let acks: Ack[] = []
let beating = false
let failures = 0
let shown: string | undefined
const seen = new Set<string>()
let results: OpResult[] = []
const opsSeen = new Set<string>()
let commands: unknown[] | null = null
let commandsSig = ''
let commandsSentAt = -Infinity

// Where the dispatcher keeps its state, as dispatcher.default_state_path
// decides it: %LOCALAPPDATA%, $XDG_STATE_HOME, or ~/.local/state, then
// imperatorium (or ccontrol, before the 2026-10-20 rename).
async function readToken($: Engine): Promise<string | null> {
  try {
    return await findToken($)
  } catch {
    // The mod is unloading, or the environment cannot be read: no token now.
    return null
  }
}

async function findToken($: Engine): Promise<string | null> {
  // A test dispatcher's own token, set by whoever started the session.
  const named = await $.env.get('IMPERATORIUM_MOD_TOKEN_FILE')
  if (named) {
    try {
      const got = String(await $.fs.read(named)).trim()
      return got.length >= 32 ? got : null
    } catch {
      return null
    }
  }
  const bases: string[] = []
  const local = await $.env.get('LOCALAPPDATA')
  if (local) bases.push(local)
  const xdg = await $.env.get('XDG_STATE_HOME')
  if (xdg) bases.push(xdg)
  const home = (await $.env.get('HOME')) || (await $.env.get('USERPROFILE'))
  if (home) bases.push(home + '/.local/state')
  for (const base of bases) {
    for (const name of ['imperatorium', 'ccontrol']) {
      try {
        const got = String(await $.fs.read(base + '/' + name + '/mod-token')).trim()
        if (got.length >= 32) {
          await readEndpoint($, base + '/' + name + '/mod-endpoint')
          return got
        }
      } catch {
        // Not there: try the next place.
      }
    }
  }
  return null
}

// Where the dispatcher that wrote the token listens (it writes this beside
// it at start), so a port other than 8792 is found. Only a loopback http
// address is taken: the mod's token goes wherever this points.
async function readEndpoint($: Engine, file: string) {
  if (endpointFixed) return
  try {
    const got = String(await $.fs.read(file)).trim()
    if (/^http:\/\/(127\.0\.0\.1|localhost|\[::1\]):\d{1,5}$/.test(got)) endpoint = got
  } catch {
    // An older dispatcher writes none: the default stands.
  }
}

async function post($: Engine, path: string, body: unknown): Promise<unknown> {
  if (!token) token = await readToken($)
  if (!token) return null
  let res
  try {
    res = await Promise.race([
      $.http.fetch(endpoint + path, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'X-Imperatorium-Mod': token },
        body: JSON.stringify(body),
      }),
      $.clock.sleep(POST_TIMEOUT_MS).then(() => null),
    ])
  } catch {
    return null
  }
  if (!res) return null
  if (res.status === 403) {
    // A new token (the dispatcher's state folder was reset): read it again.
    token = null
    return null
  }
  if (!res.ok) return null
  try {
    return JSON.parse(res.text)
  } catch {
    return null
  }
}

// What the dispatcher last said about this session, for /imperatorium only.
// Nothing goes in the terminal's status line: the window is where people
// look (2026-11-24, the user's call). A module hot-reloaded over one that
// did write there clears it once (see session.start).
function show(_$: Engine, text: string | undefined) {
  shown = text
}

async function beat($: Engine) {
  if (beating || !sessionId) return
  beating = true
  try {
    await beatOnce($)
  } catch {
    // Unloading mid-beat: the next module's first beat starts over.
  } finally {
    beating = false
  }
}

// The session's id now: a /clear goes on under a new id and fires no
// session.start, so the id read there goes stale (2026-11-21: a session
// /cleared at its start beat under its old id for hours, so the dispatcher
// had no mod for it and sent the window's prompts framed as a peer's).
async function currentId($: Engine) {
  const now = await $.session.id().catch(() => sessionId)
  if (now) sessionId = now
  return sessionId
}

async function beatOnce($: Engine) {
  await currentId($)
  {
    const sending = acks
    acks = []
    const reporting = stopped
    stopped = []
    const body: Record<string, unknown> = {
      session_id: sessionId,
      cwd,
      busy,
      turn_started: turnStarted,
      interactive,
      surfaces,
      version,
      mod_version: MOD_VERSION,
      acks: sending,
    }
    if (reporting.length) body.stopped = reporting
    const answering = results
    results = []
    if (answering.length) body.results = answering
    const listing = await commandsToSend($)
    if (listing) body.commands = listing
    if (usageDirty && usage) {
      body.usage = usage
      usageDirty = false
    }
    const reply = (await post($, '/api/mod/beat', body)) as BeatReply | null
    if (!reply || !reply.ok) {
      // Not delivered: keep the acks and the usage for the next beat.
      acks = sending.concat(acks)
      stopped = reporting.concat(stopped)
      results = answering.concat(results)
      if (listing) commandsSentAt = -Infinity
      if (body.usage) usageDirty = true
      failures += 1
      if (failures >= 3) show($, undefined)
      return
    }
    failures = 0
    show($, reply.status ? 'Imperatorium · ' + reply.status : undefined)
    if (reply.stop) stop($, reply.stop)
    for (const offered of reply.deliver || []) deliver($, offered)
    for (const op of reply.ops || []) runOp($, op)
  }
}

// The session's slash commands when they changed, or once a minute.
async function commandsToSend($: Engine): Promise<unknown[] | null> {
  const now = await $.clock.now()
  if (commands && now - commandsSentAt < COMMANDS_MS) return null
  try {
    const list = (await $.command.list()).map(c => ({
      name: c.name, description: c.description, source: c.source,
    }))
    const sig = JSON.stringify(list)
    if (sig === commandsSig && now - commandsSentAt < COMMANDS_MS) return null
    commands = list
    commandsSig = sig
    commandsSentAt = now
    return list
  } catch {
    return null
  }
}

// What the session's context holds, as /context breaks it down, estimated
// locally (no token-count requests); the grid itself is the terminal's.
async function contextNow($: Engine) {
  const got = await $.session.usage({ breakdown: 'summary' })
  const b = got.context.breakdown
  return {
    context: { tokens: got.context.tokens, window: got.context.window, percent: got.context.percent },
    breakdown: b && {
      categories: b.categories, totalTokens: b.totalTokens, maxTokens: b.maxTokens,
      percentage: b.percentage, model: b.model, memoryFiles: b.memoryFiles, mcpTools: b.mcpTools,
    },
  }
}

async function statusNow($: Engine) {
  const quiet = <T>(p: Promise<T>) => p.catch(() => null)
  const id = await quiet($.session.identity())
  const tools = (await quiet($.tool.list())) || []
  const servers = [...new Set(tools.map(t => /^mcp__(.+?)__/.exec(t.name)?.[1]).filter(Boolean))]
  return {
    version: await quiet($.session.version()),
    model: await quiet($.session.model()),
    cwd: await quiet($.session.cwd()),
    root: await quiet($.session.root()),
    repo: await quiet($.session.repo()),
    identity: id && !('deny' in id && id.deny) ? id : null,
    surfaces: await quiet($.session.surfaces()),
    mcp_servers: servers,
    mod_version: MOD_VERSION,
  }
}

async function opData($: Engine, op: Op): Promise<unknown> {
  const args = op.args || {}
  switch (op.op) {
    case 'command': {
      const got = await $.command.run({ command: String(args.command), args: String(args.args ?? '') })
      return { text: got.text }
    }
    case 'context':
      return contextNow($)
    case 'usage':
      return { ...(await $.session.usage()), model: await $.session.model() }
    case 'status':
      return statusNow($)
    case 'config':
      return { rows: await $.config.list() }
    case 'config_set': {
      const got = await $.config.set({ key: String(args.key), value: args.value as never })
      if (got && 'deny' in got && got.deny) throw new Error(String(got.deny))
      return got
    }
    case 'settings':
      return {
        permissions: await $.settings.read({ key: 'permissions' }).catch(() => null),
        hooks: await $.settings.read({ key: 'hooks' }).catch(() => null),
      }
    default:
      throw new Error('unknown op ' + op.op)
  }
}

// Runs one op, once: a command offered again is never run twice.
function runOp($: Engine, op: Op) {
  if (opsSeen.has(op.id)) return
  opsSeen.add(op.id)
  opData($, op).then(
    data => { results.push({ id: op.id, ok: true, data }); void beat($) },
    (error: unknown) => {
      results.push({ id: op.id, ok: false, error: error instanceof Error ? error.message : String(error) })
      void beat($)
    },
  )
}

// Ends the main loop's running turn, once per request: its tools stop, as
// with Esc at the terminal. Nothing running is said, not an error.
function stop($: Engine, id: string) {
  if (stopsSeen.has(id)) return
  stopsSeen.add(id)
  if (!busy || !turnId) {
    stopped.push({ id, outcome: 'idle' })
    void beat($)
    return
  }
  $.turn.abort({ turnId }).then(
    () => { stopped.push({ id, outcome: 'aborted' }); void beat($) },
    (error: unknown) => { stopped.push({ id, outcome: 'failed', reason: String(error) }); void beat($) },
  )
}

function deliver($: Engine, offered: Offered) {
  if (seen.has(offered.id)) {
    // Offered again because our acceptance never arrived: say it again.
    acks.push({ id: offered.id, outcome: 'accepted' })
    return
  }
  seen.add(offered.id)
  acks.push({ id: offered.id, outcome: 'accepted' })
  const args = offered.as_user
    ? { text: offered.text, asUser: true as const }
    : { text: offered.text }
  // Not awaited: it resolves when the prompt's turn starts, once the session
  // is idle, which may be long after this beat.
  $.prompt.submit(args).then(
    (result: { drop?: string } | undefined) => {
      acks.push(result && result.drop
        ? { id: offered.id, outcome: 'dropped', reason: result.drop }
        : { id: offered.id, outcome: 'submitted' })
    },
    (error: unknown) => {
      acks.push({ id: offered.id, outcome: 'dropped', reason: String(error) })
    },
  )
}

async function measure($: Engine) {
  try {
    const got = await $.session.usage()
    usage = { ...got, model: await $.session.model() }
    usageDirty = true
  } catch {
    // No figures yet: the next measurement brings them.
  }
}

export const register: Register = (on, options) => {
  const configured = (options as Record<string, unknown>).endpoint
  if (typeof configured === 'string' && /^https?:\/\//.test(configured)) {
    endpoint = configured.replace(/\/+$/, '')
  }

  on('session.start', async ($, e, next) => {
    // A test dispatcher elsewhere, for this session only.
    const override = await $.env.get('IMPERATORIUM_MOD_ENDPOINT').catch(() => undefined)
    if (override && /^https?:\/\//.test(override)) {
      endpoint = override.replace(/\/+$/, '')
      endpointFixed = true
    }
    sessionId = await $.session.id()
    cwd = e.cwd
    interactive = e.isInteractive
    surfaces = await $.session.surfaces()
    version = (await $.session.version()).version
    await measure($)
    $.ui.status(undefined)
    await $.command.register({
      name: 'imperatorium',
      description: 'Show what the Imperatorium dispatcher knows about this session',
      immediate: true,
    })
    $.clock.every(BEAT_MS, () => {
      void beat($)
    })
    void beat($)
    return next(e)
  })

  on('command.run', { command: 'imperatorium' }, async ($, e) => {
    if (!token) token = await readToken($)
    if (!token) {
      return { text: 'Imperatorium: no dispatcher token found - is the dispatcher installed and running?' }
    }
    const lines = [
      'Imperatorium mod ' + MOD_VERSION + ' · dispatcher ' + endpoint,
      failures ? 'not answering (' + failures + ' beats missed)' : 'connected',
      shown ? shown : 'nothing waiting',
    ]
    return { text: lines.join('\n') }
  })

  on('turn.start', async ($, e, next) => {
    if (e.agentId === undefined) {
      busy = true
      turnId = e.turnId
      turnStarted = await $.clock.now()
      void beat($)
    }
    return next(e)
  })

  on('turn.complete', async ($, e, next) => {
    const result = await next(e)
    if (e.agentId === undefined) {
      busy = false
      turnId = null
      turnStarted = null
      await measure($)
      void beat($)
    }
    return result
  })

  on('session.measure', async ($, e, next) => {
    usage = { context: e.context, rateLimits: e.rateLimits, cost: e.cost,
              model: await $.session.model() }
    usageDirty = true
    return next(e)
  })

  // The permission policy, for the calls the session would put to the
  // person. Its verdict is the dispatcher's policy.decide, the one the
  // PermissionRequest hook runs, and only where auto-approval is switched on.
  on('tool.check', async ($, e, next) => {
    const base = await next(e)
    if (base.decision !== 'ask' || PERSON_TOOLS.has(e.tool)) return base
    const asked = post($, '/api/mod/check', {
      session_id: await currentId($),
      cwd: await $.session.cwd(),
      tool_name: e.tool,
      tool_input: e.input,
      agent_id: e.agentId,
      read_only: e.isReadOnly === true,
    })
    const got = (await Promise.race([
      asked,
      $.clock.sleep(CHECK_WAIT_MS).then(() => null),
    ])) as CheckReply | null
    if (got && (got.verdict === 'allow' || got.verdict === 'deny')) {
      return { decision: got.verdict, reason: 'Imperatorium policy: ' + (got.reason || got.verdict) }
    }
    return base
  }).catch(($, e, next) => next(e))
}
