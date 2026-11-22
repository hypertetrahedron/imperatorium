# Research: a "super agent" over every running session

2026-11-20. This answers a request: should Imperatorium have a super agent that can see every running session and that you could ask for the other agents' status, or ask to watch them? Are there existing solutions, other methods or related use cases?

Method: one pass over the Imperatorium codebase and two web-research passes, one on products and one on the research and security literature. The Claude Code docs pages (agent view, agent teams, cross-session messaging, Projects) were read directly. Most other product and paper claims come from search summaries; anything not confirmed is marked *(unverified)*.

## Verdict

**Yes, but only a narrow version: read-only, on-demand, working from structured state, acting only through what Imperatorium already does. Do not build an always-on overseer that reads raw transcripts and acts on sessions by itself.**

Asking "what is everyone doing, who is blocked, what finished while I was away?" in plain language is a real gap. No first-party product gives you a conversation over sessions it did not start itself. Imperatorium already holds the structured state such an agent would need, which most tools lack. The risky part is the second half of the idea, an agent that *acts on* other agents. Both the literature and every vendor converge on keeping that narrow.

## What exists

| | Overseer | Relevant to us |
|---|---|---|
| **Claude Code agent view** (`claude agents`, May 2026, 2.1.139+) | Deterministic UI; Haiku writes only the one-line row summaries | First-party fleet board: dispatch, peek, reply, stop. Replying from the peek panel does **not** answer permission dialogs. Native Windows support not stated *(unverified)*. Overlaps the board Imperatorium already has. |
| **Cross-session messaging** (`ListAgents`, `SendMessage`, `notify_when_idle`, 2.1.224+) | LLM to LLM | Any session can already list and message your other local sessions (a named pipe on Windows). Text only: no transcript reads. A message can never approve anything or change config. `notify_when_idle` is a free push notice. |
| **Agent teams** (experimental) | LLM lead | Manages only teammates it spawned. Teammates inherit the lead's permission mode, bypass included, which Anthropic calls a pitfall. Anthropic's advice is to try subagents or messaging first. |
| **Claude Code Projects** (claude.ai/code, beta) | LLM coordinator chat | The closest to "ask a manager": an Overview of threads waiting on you or ready for review. Manages only the threads it starts, in the cloud. |
| **Devin "Managed Devins", Factory Missions, Replit Missions, Managed Agents multiagent** | LLM coordinator | All of these manage workers they spawned themselves, with budget caps. Factory admits its orchestrator "scopes too broadly sometimes". |
| **Cursor 3 Agents Window, Codex app, GitHub Agent HQ, Antigravity Agent Manager, Warp, Amp** | Deterministic UI | The common pattern: the human is the coordinator and the UI shows status. |
| **claudectl, superharness, thin-supervisor, pi-claude-supervisor** | LLM or policy supervisor | Auto-approve or unstick workers; tmux-based. Approving on the human's behalf is exactly what Anthropic forbids peer messages from doing. |
| **MCP session servers** (claude-intercom, cc-tap, claude-session, Aegis, claude-code-bridge-mcp) | A tool surface for any LLM | List, read and send, which turns any Claude session into the "super agent". This is the cheapest way to build one. Small projects, mostly tmux, with little safety documented *(details unverified)*. |

Already rejected in `ROADMAP.md` (Deferred): Claude Squad, ccmanager, Uzi, dmux (WSL/tmux), Conductor (macOS), amux. Vibe Kanban's company shut down on 2026-04-10.

**Patterns:**
- No product has a conversational overseer that *adopts* existing local sessions.
- Fleet views are deterministic, with an LLM only for summary strings.
- Every vendor refuses to let agent-to-agent messages count as consent.
- Reliable status comes from hooks and event files, which Imperatorium has. Pane scraping, which most open-source tools use, is fragile.

## What the evidence says

**Architecture.**
- Anthropic: multi-agent runs use about 15x the tokens of chat, and single agents about 4x. Multi-agent pays when the work is wide and read-heavy, and is a poor fit where context is shared or dependencies are dense.
- Cognition ("Don't Build Multi-Agents", and its April 2026 follow-up, *details secondary*): parallel *writers* diverge because each lacks the others' implicit decisions. Read-only and review agents work.
- Kim et al. 2025 (180 configurations): centralised orchestration contains error amplification (about 4.4x, against 17.2x for independent agents). It hurts sequential planning by 39-70%.
- MAST (Cemri et al.): multi-agent failures are structural (specification, inter-agent misalignment, verification), not just weak models.
- So a single agent answering questions over compact state is the favourable case. One that dispatches into sessions holding context it lacks is the unfavourable one.

**Security: the main constraint.**
- A super agent would read the output of 20 sessions: web pages, repository files, command output. That is untrusted by construction. If it can also dispatch, it becomes a channel for an injection in one session to reach every other.
- That is the published failure pattern:
  - Prompt Infection (Lee & Tiwari 2024): self-replicating injection across agents.
  - OWASP Agentic Top 10 (Dec 2025): ASI01 goal hijack, ASI07 inter-agent communication, ASI08 cascading failures *(secondary coverage)*.
  - Willison's lethal trifecta and Meta's Rule of Two: never combine untrusted input, sensitive access, and state change or exfiltration.
- Defences that fit:
  - Context minimisation: feed it fields, not transcripts.
  - Dual LLM / quarantine: a tool-less summariser for any free text.
  - Action-Selector: a fixed menu of actions, confirmed by you.
  - CaMeL, the strongest of these, solved 77% of AgentDojo tasks with provable security, against 84% with no defence.
- Imperatorium is already on the right side of this. A dispatched prompt arrives as a peer message with no authority, the policy hook refuses changes to its own machinery, and Send As Typed only works from the window.

**Human factors.**
- Every practitioner account names the same bottleneck: *review and attention*, not agent speed.
- Two warnings apply to a super agent:
  - A fluent "all fine" report breeds automation complacency.
  - A chatty overseer adds to the interruptions it was meant to reduce.
- Reports should cite the session, event and time they rest on, so a claim can be checked in one click.

**Cost.** An LLM that polls 20 sessions pays for every unchanged snapshot. Imperatorium is event-driven already (hooks, SSE), so a model should wake only when you ask or on a meaningful change. Put the stable prompt and tool definitions first for caching, and the state last. Haiku or Sonnet is enough for summarising structured state (judgement, not benchmarked).

## Recommendation, in steps

0. **Try it with nothing built (this week).** The CLI and API already answer everything: `imperatorium sessions --json`, `/api/matrix`, `/api/events`, `/api/session_status`, `/api/awaited`, `/api/conversation`. Write a skill, e.g. `/fleet`, that tells a Claude session how to read them, open a "control" session, and ask it questions for a few days. That shows which questions you actually ask before anything is built.

1. **`imperatorium mcp`: read-only fleet tools.** Tools: `list_sessions`, `attention` (who waits, on what, how long), `session_status`, `recent_events`, `spend`, `changes_overview`, and `conversation_tail` (bounded, marked untrusted). Ship it with the planned user-scope plugin (ROADMAP: "Ship hooks as a user-scope plugin", which already names an MCP server). Any session can then be the super agent, through a first-party extension point, with no new runtime. Returns are structured fields first; free text is truncated, wrapped and labelled as data.

2. **"Ask" in the window.** A prompt box that runs a short-lived `claude -p` (Haiku or Sonnet) over a snapshot the dispatcher assembles, with no tools at all. The answer cites session ids that link to their tiles. On demand only; no background loop.

3. **Monitoring as deterministic watches, which the LLM may *write* but never *run*.**
   - "Tell me when gem-trip finishes", "ping me if anything waits more than 10 minutes", "warn me if a session passes $5": the agent turns each sentence into a watch rule that you confirm, and the dispatcher evaluates it on hook events with no model in the loop.
   - It reuses the alerts, notify (ntfy, speech) and Stream Deck paths already there. This is the "monitor" half of the idea, without an always-on LLM.

4. **Actions, if ever, through the existing doors only.**
   - A fixed menu: queue a prompt, stop a turn, compact, open the session. Each goes out as a peer-framed dispatch after you confirm it.
   - Never: answering permission prompts or questions, touching `policy.json` or settings, Send As Typed, or acting because a transcript said to.
   - The overseer gets no outbound network tools.
   - Every action is logged next to `decisions.jsonl`.

Before step 2, check whether `claude agents` (agent view) runs on native Windows and how it overlaps the board. Its Haiku row summaries already do what the tile's `doing` line does.

## Other use cases worth considering

Most of these follow from the same read-only, fleet-wide view. Several are already on the roadmap.

- **While-you-were-away digest**: what each session finished, what it is waiting on, what it spent. The single most valuable output of an LLM over this state.
- **Triage ranking**: order the "waiting for you" list by cost of delay (cache about to go cold, blocked on a one-word answer, long-running build done) rather than by arrival time.
- **Cross-session conflict warnings**: Project Changes already finds file overlaps between worktrees. Raise them as an alert when two live sessions start editing the same files.
- **Stuck or looping detection**: the same tool failure repeated, no progress for N minutes, or context near full. Mostly deterministic, from the activity timeline and errors data.
- **Budget governor / shared work queue** (Deferred, survey item 7): start queued background work only while 5h/7d usage stays under a cap. The super agent could *propose* the queue's order.
- **Broadcast with review** (Deferred, survey item 8): "commit and push" or "compact before the cache goes cold" sent to a selected set, shown as a preview first.
- **Knowledge relay**: session A found something session B needs (a fixed dependency, a changed API). Suggest a peer message rather than sending it.
- **Review assistant for finished worktrees**: summarise what a background session's branch would bring before you merge it from the Worktrees tab.
- **Voice status**: "what's going on?" through the existing dictation path, with the answer spoken through the speech channel.
- **Weekly insights narrative** over the existing errors and costs data.

## Sources

Claude Code docs: [agent view](https://code.claude.com/docs/en/agent-view) · [agent teams](https://code.claude.com/docs/en/agent-teams) · [cross-session messaging](https://code.claude.com/docs/en/cross-session-messaging) · [Projects](https://code.claude.com/docs/en/claude-projects.md) · [monitoring (OTel)](https://code.claude.com/docs/en/monitoring-usage) · [Managed Agents multiagent](https://platform.claude.com/docs/en/managed-agents/multi-agent) · [prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)

Architecture: [Anthropic multi-agent research system](https://www.anthropic.com/engineering/multi-agent-research-system) · [Building effective agents](https://www.anthropic.com/engineering/building-effective-agents) · [Cognition, Don't Build Multi-Agents](https://cognition.ai/blog/dont-build-multi-agents) · [Cognition, Multi-Agents: What's Actually Working](https://cognition.ai/blog/multi-agents-working) · [MAST](https://arxiv.org/abs/2503.13657) · [Scaling Agent Systems](https://arxiv.org/abs/2512.08296) · [LangChain multi-agent benchmarks](https://blog.langchain.com/benchmarking-multi-agent-architectures)

Security: [Lethal trifecta](https://simonwillison.net/2025/Jun/16/the-lethal-trifecta/) · [Meta, Agents Rule of Two](https://ai.meta.com/blog/practical-ai-agent-security/) · [Prompt Infection](https://arxiv.org/abs/2410.07283) · [OWASP Agentic Top 10](https://genai.owasp.org/2025/12/09/owasp-top-10-for-agentic-applications-the-benchmark-for-agentic-security-in-the-age-of-autonomous-ai/) · [Design Patterns for Securing LLM Agents](https://arxiv.org/abs/2506.08837) · [CaMeL](https://arxiv.org/abs/2503.18813)

Products: [Devin manages Devins](https://cognition.ai/blog/devin-can-now-manage-devins) · [Factory Missions](https://factory.ai/news/missions-architecture) · [Cursor 3](https://cursor.com/blog/cursor-3) · [Codex app](https://openai.com/index/introducing-the-codex-app/) · [GitHub Agent HQ](https://github.blog/news-insights/company-news/welcome-home-agents/) · [Antigravity](https://developers.googleblog.com/build-with-google-antigravity-our-new-agentic-development-platform/) · [Vibe Kanban shutdown](https://vibekanban.com/blog/shutdown) · [claudectl](https://docs.rs/crate/claudectl/0.47.0/source/docs/index.md) · [cc-tap](https://glama.ai/mcp/servers/es617/cc-tap)

Human factors: [Superset, roadmap to 100 agents](https://superset.sh/blog/roadmap-to-100-agents) · ["You can parallelize the typing, you can't parallelize the trust"](https://dev.to/agentiknet/you-can-parallelize-the-typing-you-cant-parallelize-the-trust-2fkd) · [Ask HN: agent orchestrators](https://hn.svelte.dev/item/46993479)
