import { expect, mock, test } from 'claude-code/testing'
import type { On } from 'claude-code'

const TOKEN = 'x'.repeat(40)

type Sent = { path: string; body: Record<string, unknown>; headers: Record<string, string> }

// The world beneath the mod: a session, a token on disk, and a dispatcher
// whose answers each test chooses. Returns what the mod sent and submitted.
function world(on: On, reply: (path: string, body: Record<string, unknown>) => unknown,
               opts: { token?: boolean; id?: () => string; usage?: (e: { breakdown?: string }) => unknown } = {}) {
  const sent: Sent[] = []
  const submitted: { text: string; asUser?: true }[] = []
  const status: (string | undefined)[] = []
  mock.env(on, { LOCALAPPDATA: 'C:/state' })
  on('fs.read', (_, e) => {
    // The engine hands a hook the path in the platform's own form.
    if (opts.token !== false && String(e.path).replace(/\\/g, '/') === 'C:/state/imperatorium/mod-token') {
      return { value: TOKEN + '\n' }
    }
    throw new Error('no such file')
  })
  on('session.id', () => ({ value: opts.id ? opts.id() : 'sess-1' }))
  on('session.cwd', () => ({ value: 'C:/proj' }))
  on('session.surfaces', () => ({ value: ['terminal'] }))
  on('session.version', () => ({ value: { version: '2.1.317' } }))
  on('session.model', () => ({ value: 'claude-opus-5-5' }))
  on('session.usage', (_, e) => opts.usage ? { value: opts.usage(e) as never } : ({
    value: { startedAt: 0, context: { tokens: 1000, window: 200000, percent: 0.5 },
             rateLimits: [{ kind: 'five_hour', percentUsed: 12 }], cost: { usd: 0.25 } },
  }))
  on('command.register', () => ({ value: { command: 'imperatorium' } }))
  on('ui.status', (_, e) => {
    status.push(e.text)
    return { value: undefined }
  })
  on('prompt.submit', (_, e) => {
    submitted.push({ text: e.text, ...(e.origin && 'asUser' in e.origin && e.origin.asUser ? { asUser: true as const } : {}) })
    return { text: e.text }
  })
  on('http.fetch', (_, e) => {
    const body = JSON.parse(String(e.init?.body ?? '{}'))
    const path = String(e.url).replace('http://127.0.0.1:8792', '')
    sent.push({ path, body, headers: e.init?.headers ?? {} })
    const answer = reply(path, body)
    return { value: { status: 200, ok: true, headers: {}, text: JSON.stringify(answer) } }
  })
  return { sent, submitted, status }
}

test('a prompt from the window goes in as the person; others go in framed', async ($, on) => {
  const clock = mock.clock(on)
  let offered = false
  const w = world(on, path => {
    if (path !== '/api/mod/beat') return {}
    if (offered) return { ok: true, deliver: [], status: null }
    offered = true
    return {
      ok: true,
      status: '2 waiting on you: api, web',
      deliver: [
        { id: 'p1', text: 'run the tests', as_user: true },
        { id: 'p2', text: '[from impctl] tidy up', as_user: false },
      ],
    }
  })
  on('session.start', (_, e) => ({ cwd: e.cwd }))

  await $.session.start({ cwd: 'C:/proj', surface: 'terminal', isInteractive: true })
  await clock.settle()

  const first = w.sent.find(s => s.path === '/api/mod/beat')
  expect(first?.headers['X-Imperatorium-Mod']).toBe(TOKEN)
  expect(first?.body.session_id).toBe('sess-1')
  expect(first?.body.busy).toBe(false)
  expect((first?.body.usage as { cost: { usd: number } }).cost.usd).toBe(0.25)

  expect(w.submitted).toEqual([
    { text: 'run the tests', asUser: true },
    { text: '[from impctl] tidy up' },
  ])
  expect(w.status).toContain('Imperatorium · 2 waiting on you: api, web')

  // The next beat says both were accepted, then started.
  await clock.advance(2000)
  const acks = w.sent.filter(s => s.path === '/api/mod/beat')
    .flatMap(s => (s.body.acks as { id: string; outcome: string }[]))
  expect(acks).toContainEqual({ id: 'p1', outcome: 'accepted' })
  expect(acks).toContainEqual({ id: 'p2', outcome: 'accepted' })
  expect(acks).toContainEqual({ id: 'p1', outcome: 'submitted' })
})

test('after a /clear the beats carry the new session id', async ($, on) => {
  // A /clear goes on under a new id and fires no session.start.
  const clock = mock.clock(on)
  let id = 'sess-1'
  const w = world(on, () => ({ ok: true }), { id: () => id })
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  await $.session.start({ cwd: 'C:/proj', surface: 'terminal', isInteractive: true })
  await clock.settle()
  id = 'sess-2'
  await clock.advance(2000)

  const ids = w.sent.filter(s => s.path === '/api/mod/beat').map(s => s.body.session_id)
  expect(ids[0]).toBe('sess-1')
  expect(ids[ids.length - 1]).toBe('sess-2')
})

test('the same prompt offered twice is submitted once', async ($, on) => {
  const clock = mock.clock(on)
  const w = world(on, () => ({ ok: true, deliver: [{ id: 'p1', text: 'once', as_user: true }] }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))

  await $.session.start({ cwd: 'C:/proj', surface: 'terminal', isInteractive: true })
  await clock.settle()
  await clock.advance(2000)
  await clock.advance(2000)

  expect(w.submitted.length).toBe(1)
})

test('with no dispatcher token the mod sends nothing', async ($, on) => {
  const clock = mock.clock(on)
  const w = world(on, () => ({ ok: true }), { token: false })
  on('session.start', (_, e) => ({ cwd: e.cwd }))

  await $.session.start({ cwd: 'C:/proj', surface: 'terminal', isInteractive: true })
  await clock.advance(6000)

  expect(w.sent).toEqual([])
})

test('the policy decides what the session would ask about', async ($, on) => {
  mock.clock(on)
  const w = world(on, path => (path === '/api/mod/check'
    ? { verdict: 'allow', reason: 'read-only command' }
    : { ok: true }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('tool.check', () => ({ decision: 'ask' as const }))
  await $.session.start({ cwd: 'C:/proj', surface: 'terminal', isInteractive: true })

  const got = await $.tool.check({ tool: 'Bash', input: { command: 'git status' } })

  expect(got.decision).toBe('allow')
  expect(got.reason).toBe('Imperatorium policy: read-only command')
  const asked = w.sent.find(s => s.path === '/api/mod/check')
  expect(asked?.body.tool_name).toBe('Bash')
  expect(asked?.body.tool_input).toEqual({ command: 'git status' })
})

test('an allow or a deny the session already reached is left alone', async ($, on) => {
  mock.clock(on)
  const w = world(on, () => ({ verdict: 'deny', reason: 'no' }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('tool.check', () => ({ decision: 'allow' as const }))
  await $.session.start({ cwd: 'C:/proj', surface: 'terminal', isInteractive: true })

  const got = await $.tool.check({ tool: 'Read', input: { file_path: 'a.txt' } })

  expect(got.decision).toBe('allow')
  expect(w.sent.some(s => s.path === '/api/mod/check')).toBe(false)
})

test('a question to the person is never answered from here', async ($, on) => {
  mock.clock(on)
  const w = world(on, () => ({ verdict: 'allow', reason: 'x' }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('tool.check', () => ({ decision: 'ask' as const }))
  await $.session.start({ cwd: 'C:/proj', surface: 'terminal', isInteractive: true })

  const got = await $.tool.check({ tool: 'AskUserQuestion', input: { questions: [] } })

  expect(got.decision).toBe('ask')
  expect(w.sent.some(s => s.path === '/api/mod/check')).toBe(false)
})

test('a dispatcher that does not answer in time leaves the ask standing', async ($, on) => {
  const clock = mock.clock(on)
  world(on, () => ({ ok: true }))
  // A dispatcher that takes five seconds over every check.
  on('http.fetch', { url: 'http://127.0.0.1:8792/api/mod/check' }, async () => {
    await clock.sleep(5000)
    return { value: { status: 200, ok: true, headers: {}, text: '{"verdict":"allow"}' } }
  })
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('tool.check', () => ({ decision: 'ask' as const }))
  await $.session.start({ cwd: 'C:/proj', surface: 'terminal', isInteractive: true })

  const pending = $.tool.check({ tool: 'Bash', input: { command: 'make' } })
  await clock.advance(1600)

  expect((await pending).decision).toBe('ask')
})

test('a stop from the window ends the running turn and says so', async ($, on) => {
  const clock = mock.clock(on)
  let asked = 0
  const w = world(on, (path, body) => {
    if (path !== '/api/mod/beat') return {}
    // Asked while the turn runs; offered again, it is not stopped twice.
    if (body.busy && asked < 2) { asked += 1; return { ok: true, stop: 'st1' } }
    return { ok: true }
  })
  const aborted: string[] = []
  on('turn.abort', (_, e) => {
    aborted.push(e.turnId)
    return { value: undefined }
  })
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  await $.session.start({ cwd: 'C:/proj', surface: 'terminal', isInteractive: true })
  await $.turn.start({ text: 'go', turnId: 't-42' })
  await clock.advance(2000)
  await clock.advance(2000)

  expect(aborted).toEqual(['t-42'])
  const said = w.sent.filter(s => s.path === '/api/mod/beat')
    .flatMap(s => (s.body.stopped as { id: string; outcome: string }[] | undefined) ?? [])
  expect(said).toEqual([{ id: 'st1', outcome: 'aborted' }])
})

test('a stop with nothing running is said to be idle', async ($, on) => {
  const clock = mock.clock(on)
  const w = world(on, path => (path === '/api/mod/beat' ? { ok: true, stop: 'st2' } : {}))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  await $.session.start({ cwd: 'C:/proj', surface: 'terminal', isInteractive: true })
  await clock.advance(2000)

  const said = w.sent.filter(s => s.path === '/api/mod/beat')
    .flatMap(s => (s.body.stopped as { id: string; outcome: string }[] | undefined) ?? [])
  expect(said).toEqual([{ id: 'st2', outcome: 'idle' }])
})

type Result = { id: string; ok: boolean; data?: Record<string, unknown>; error?: string }
const resultsOf = (sent: Sent[]) => sent.filter(s => s.path === '/api/mod/beat')
  .flatMap(s => (s.body.results as Result[] | undefined) ?? [])

test('a command from the window runs as typed, once, and its output goes back', async ($, on) => {
  const clock = mock.clock(on)
  const w = world(on, path => (path === '/api/mod/beat'
    ? { ok: true, ops: [{ id: 'o1', op: 'command', args: { command: 'model', args: 'sonnet' } }] }
    : {}))
  const ran: string[] = []
  on('command.run', (_, e) => {
    ran.push(e.command + ' ' + e.args)
    return { text: 'Set model to Sonnet 5.5' }
  })
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  await $.session.start({ cwd: 'C:/proj', surface: 'terminal', isInteractive: true })
  await clock.advance(2000)
  await clock.advance(2000)

  // Offered on every beat here, it still ran only once.
  expect(ran).toEqual(['model sonnet'])
  expect(resultsOf(w.sent)).toEqual([{ id: 'o1', ok: true, data: { text: 'Set model to Sonnet 5.5' } }])
})

test('the session\'s slash commands go with its first beat', async ($, on) => {
  const clock = mock.clock(on)
  const w = world(on, () => ({ ok: true }))
  on('command.list', () => ({ value: [
    { name: 'compact', description: 'Free up context', source: 'builtin' },
    { name: 'deploy', description: 'Ship it', source: 'plugin', plugin: 'ops' },
  ] }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  await $.session.start({ cwd: 'C:/proj', surface: 'terminal', isInteractive: true })
  await clock.advance(2000)

  const beats = w.sent.filter(s => s.path === '/api/mod/beat')
  expect(beats[0].body.commands).toEqual([
    { name: 'compact', description: 'Free up context', source: 'builtin' },
    { name: 'deploy', description: 'Ship it', source: 'plugin' },
  ])
  // Unchanged within the minute: not sent again.
  expect(beats[beats.length - 1].body.commands).toBe(undefined)
})

test('a /config change the session refuses comes back as an error', async ($, on) => {
  const clock = mock.clock(on)
  const w = world(on, path => (path === '/api/mod/beat'
    ? { ok: true, ops: [{ id: 'c1', op: 'config_set', args: { key: 'theme', value: 'dark' } }] }
    : {}))
  on('config.set', () => ({ deny: 'a trusted source owns this row' }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  await $.session.start({ cwd: 'C:/proj', surface: 'terminal', isInteractive: true })
  await clock.advance(2000)
  await clock.advance(2000)

  expect(resultsOf(w.sent)).toEqual([{ id: 'c1', ok: false, error: 'a trusted source owns this row' }])
})

test('the context is read as /context breaks it down', async ($, on) => {
  const clock = mock.clock(on)
  const w = world(on, path => (path === '/api/mod/beat' ? { ok: true, ops: [{ id: 'x1', op: 'context' }] } : {}), {
    usage: e => ({ startedAt: 0, rateLimits: [],
             context: { tokens: 5000, window: 200000, percent: 2.5,
                        ...(e.breakdown ? { breakdown: { categories: [{ name: 'Messages', tokens: 4000 }],
                                                          totalTokens: 5000, maxTokens: 200000, percentage: 2.5,
                                                          gridRows: [[]], model: 'opus', memoryFiles: [], mcpTools: [] } } : {}) } }),
  })
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  await $.session.start({ cwd: 'C:/proj', surface: 'terminal', isInteractive: true })
  await clock.advance(2000)
  await clock.advance(2000)

  const got = resultsOf(w.sent).find(r => r.id === 'x1')
  expect(got?.ok).toBe(true)
  const b = (got?.data as { breakdown: Record<string, unknown> }).breakdown
  expect(b.categories).toEqual([{ name: 'Messages', tokens: 4000 }])
  expect(b.gridRows).toBe(undefined)
})
