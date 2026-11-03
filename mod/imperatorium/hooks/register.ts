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
// It also answers `tool.check` from the dispatcher's permission policy
// (POST /api/mod/check), for the calls the session's rules and mode would put
// to the person. Anything unsure, slow or broken leaves the session's own
// decision standing.
//
// Every request carries the token the dispatcher keeps in its state folder
// (`mod-token`). With no dispatcher, the mod does nothing at all.
import type { EngineInterface, Register } from 'claude-code'

type Engine = EngineInterface

const MOD_VERSION = '0.2.0'
const BEAT_MS = 2000
// How long a permission check waits for the dispatcher before leaving the
// decision to the session: a tool call should never wait on it for long.
const CHECK_WAIT_MS = 1500
// Tools a hook can only tighten (a question to the person, a plan to
// approve): an allow from here would not dismiss their dialog.
const PERSON_TOOLS = new Set(['AskUserQuestion', 'ExitPlanMode'])

type Offered = { id: string; text: string; as_user: boolean }
type Ack = { id: string; outcome: 'accepted' | 'submitted' | 'dropped'; reason?: string }
type BeatReply = { ok?: boolean; deliver?: Offered[]; status?: string | null }
type CheckReply = { verdict?: string; reason?: string }

// The module's state. A reload starts it over: the dispatcher re-offers a
// prompt that was never accepted, and the next beat restores the rest.
let endpoint = 'http://127.0.0.1:8792'
let token: string | null = null
let sessionId = ''
let cwd = ''
let busy = false
let turnStarted: number | null = null
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
        if (got.length >= 32) return got
      } catch {
        // Not there: try the next place.
      }
    }
  }
  return null
}

async function post($: Engine, path: string, body: unknown): Promise<unknown> {
  if (!token) token = await readToken($)
  if (!token) return null
  let res
  try {
    res = await $.http.fetch(endpoint + path, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-Imperatorium-Mod': token },
      body: JSON.stringify(body),
    })
  } catch {
    return null
  }
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

function show($: Engine, text: string | undefined) {
  if (text === shown) return
  shown = text
  $.ui.status(text)
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

async function beatOnce($: Engine) {
  {
    const sending = acks
    acks = []
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
    if (usageDirty && usage) {
      body.usage = usage
      usageDirty = false
    }
    const reply = (await post($, '/api/mod/beat', body)) as BeatReply | null
    if (!reply || !reply.ok) {
      // Not delivered: keep the acks and the usage for the next beat.
      acks = sending.concat(acks)
      if (body.usage) usageDirty = true
      failures += 1
      if (failures >= 3) show($, undefined)
      return
    }
    failures = 0
    show($, reply.status ? 'Imperatorium · ' + reply.status : undefined)
    for (const offered of reply.deliver || []) deliver($, offered)
  }
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
    if (override && /^https?:\/\//.test(override)) endpoint = override.replace(/\/+$/, '')
    sessionId = await $.session.id()
    cwd = e.cwd
    interactive = e.isInteractive
    surfaces = await $.session.surfaces()
    version = (await $.session.version()).version
    await measure($)
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
      turnStarted = await $.clock.now()
      void beat($)
    }
    return next(e)
  })

  on('turn.complete', async ($, e, next) => {
    const result = await next(e)
    if (e.agentId === undefined) {
      busy = false
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
      session_id: sessionId,
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
