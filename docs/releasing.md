# Releasing Imperatorium

How code gets from a branch to someone's machine. Adopted 2026-11-15, for the
first public releases of the native (Rust) build.

Imperatorium is a single binary that installs hooks into Claude Code, runs a
background service, keeps state on disk, and updates itself. That shapes the
rules below: a bad release is not a broken web page someone reloads, it is a
broken service on someone else's machine, editing their `~/.claude/settings.json`.
So the strategy optimises for three things, in order:

1. **Every published build is reproducible from a tag, by CI.** Nothing built on
   a developer machine is ever distributed.
2. **Users get a safe default.** Stable is boring; preview is opt-in.
3. **Every update can be undone.** One command back to the previous binary, and
   state that a newer version wrote never corrupts an older one.

---

## 1. Branches

Trunk-based development: one long-lived branch, short-lived branches into it.

| Branch | Lives | Purpose | Who writes to it |
|---|---|---|---|
| `main` | forever | The trunk. Always builds, always passes `check`. Previews are tagged from here. | Pull requests only |
| `feat/…`, `fix/…`, `refactor/…`, `docs/…` | days | One change each. Branch from `main`, merge back to `main`. | Anyone |
| `release/X.Y` | while X.Y is supported | Patch releases for a minor version after `main` has moved on. Created **only when needed** (see §5). | Cherry-picks from `main` |
| `exp/…` | as long as needed | Long-running experiments that may never merge (today: `herdr`, `mods`). Never released from. | The experiment's owner |

Rules:

- **`main` is protected** (GitHub ruleset): pull request required, the `check`
  workflow must pass (Windows is the gate), linear history, no force pushes,
  no deletion. As the sole maintainer you may allow yourself to bypass review
  approval, but not the status check.
- **Merge by squash or rebase**, so `main` reads as one commit per change. The
  commit subject is the release-note line, so write it for a user
  ("Permission requests stay in the window until answered"), as this repo
  already does.
- **Short-lived means short.** A branch more than ~2 weeks old gets rebased or
  split. Work that cannot be finished in that time goes in behind a config
  flag (off by default) rather than on a long branch.
- **Fixes go to `main` first**, then are cherry-picked (`git cherry-pick -x`) to
  any `release/X.Y` that needs them. Never the other way round, so a fix can't
  be lost in the next minor.
- **Worktrees** remain the way to work on several branches at once (the
  dispatcher already branches background worktrees from the session's branch).
  The worktree that runs the live dispatcher should track `main`, not a
  feature branch.

### Migration from the old layout (done 2026-11-15)

`main` had been the Python era (8de6e51, tag `v0.1.0`) while the real trunk
was `rust`, 203 commits ahead and containing `main`.

1. `main` fast-forwarded to `rust` (2625e35) and pushed; no history
   rewritten. `last-python` and `v0.1.0` keep the Python line reachable.
2. The `imperatorium-rust` worktree (the live one) now has `main` checked
   out; the `rust` branch is deleted.
3. Deleted, local and on origin: `python-out/*`, `refactor/*`,
   `rename-imperatorium`, and `mods` (its one unmerged commit was already on
   `main` as 1290730, and touched only the deleted Python).
4. `herdr` is left exactly as it is until the native port is declared a
   success; then it becomes `exp/herdr`.
5. Rulesets for `main` and for `v*` tags (§3).
6. The README on `main` is now the native one.

---

## 2. Versions

[Semantic Versioning](https://semver.org/), `MAJOR.MINOR.PATCH`, with
pre-release tags for previews: `0.4.0-preview.1`, `0.4.0-preview.2`, then
`0.4.0`. `update.rs` already orders these correctly.

**While at 0.x** (the next few months):

- **Minor** (`0.4` → `0.5`): anything that breaks a contract (below), and new
  features generally.
- **Patch** (`0.4.0` → `0.4.1`): fixes only. No new config keys, no state
  changes, no new prerequisites. A user should be able to take a patch
  without reading anything.

**1.0** is a promise, not a milestone of features: declare it when the
contracts below are stable enough to keep. After 1.0, breaking a contract
means a major version.

### What counts as a contract

A change to any of these is *breaking* and needs a minor bump (0.x) or major
bump (≥1.0), a migration, and a line in the release notes' **Breaking**
section:

| Contract | Where it lives |
|---|---|
| `config.json` / `policy.json` keys and meanings | `config.example.json`, `policy.example.json` |
| The state folder's layout and file formats | `%LOCALAPPDATA%\imperatorium`, `$XDG_STATE_HOME` or `~/.local/state/imperatorium` |
| Hook commands written into `~/.claude/settings.json` | `install.rs` |
| The minimum Claude Code version | `install.rs` prerequisites |
| The HTTP API (`/api/*`) that the window, Stream Deck and scripts use | `server/` |
| The `remote-channel` protocol between the dispatcher and a host's binary | `remote.rs` |
| The CLI's subcommands and flags | `cli.rs` |
| The release layout: `manifest.json`, asset names, `SHA256SUMS` | `release.rs` — **old installed binaries read this to update themselves, so it can only ever grow** |

The last row is special: an installed 0.4.0 will read the manifest of every
future release. Add fields; never rename or remove one.

### Where the version comes from

`rust/imperatorium/Cargo.toml` is the source of truth.

- On `main` it reads the **next** version with `-dev`: `0.4.0-dev`. (`dev`
  sorts below `preview`, so a dev build is never "newer" than a published
  preview of the same version.)
- A **preview** is tagged straight from `main` with no version commit:
  `v0.4.0-preview.3`. The workflow checks that the tag's `0.4.0` matches
  Cargo's `0.4.0-dev` and stamps the full version in through
  `IMPERATORIUM_BUILD_VERSION`, as it does today.
- A **stable** release gets a release commit setting Cargo to exactly `0.4.0`
  (and `Cargo.lock`, and `CHANGELOG.md`), tagged `v0.4.0`. The workflow
  refuses a stable tag whose Cargo version differs. The next commit on `main`
  bumps to `0.5.0-dev`.

### Numbering from here

`v0.1.0` is the Python line. The `0.3.x` numbers were used by the local mock
releases in `dist/` during phase 6, and a machine that installed one of those
would compare against them. Start the public Rust line at **0.4.0** so no
published version collides with anything already installed somewhere.

---

## 3. Tags

- **Annotated** (`git tag -a`), never lightweight. The annotation is the
  release's "what's new", which the window's update panel shows.
- **Format:** `vX.Y.Z` or `vX.Y.Z-preview.N`. Nothing else starting with `v`.
- **Immutable.** Never move or delete a pushed release tag. A bad release is
  fixed by a new version, not by re-tagging (old binaries and caches have
  already seen the old one).
- **Only from `main`** (previews and minor/major stables) **or `release/X.Y`**
  (patches). The release workflow checks the tagged commit is on one of them.
- **Protected** by a tag ruleset: only you can create `v*`; no updates or
  deletions.
- Non-release tags (`last-python`) don't start with `v`.

---

## 4. Channels

Two, as `update.rs` and the installers already implement:

| Channel | Gets | From | Who |
|---|---|---|---|
| **stable** (default) | `vX.Y.Z` | GitHub's "latest" release | Everyone |
| **preview** | `vX.Y.Z-preview.N` | the rolling `preview` release | Opt-in testers, and **your own machines** |

- **Dogfood the preview channel.** Your live dispatcher should run published
  preview builds installed by `imperatorium update --channel preview`, not a
  binary built from a worktree. That way the exact bytes users will get have
  run on a real machine first. Day-to-day development builds go in a second
  install beside it (`--task-prefix`, `--port`, `--state`, as the tests do).
- **Promotion, not rebuild.** A stable release is cut from the same commit as
  a preview that has soaked (plus the release commit, which only changes
  version strings and the changelog). Nothing lands in a stable that never
  shipped as a preview.
- **Soak:** at least 3 days on preview, across at least one real Windows
  machine and one remote Linux host, with no open regression.

---

## 5. Cadence and support

- **Previews:** whenever `main` has something worth trying. Cheap — just a tag.
- **Stable minors:** every 2–4 weeks, or when a preview has soaked and has
  something users want. No fixed calendar; there is one maintainer.
- **Patches:** as needed, for regressions and security.
- **Supported versions:** while at 0.x, only the **latest stable minor** gets
  fixes. Users are expected to update; `imperatorium update` makes that one
  command. After 1.0: the latest minor, plus security fixes for the one
  before it for 30 days after a new minor ships.

### Patching an older minor

Usually unnecessary: if `main` is releasable, ship the fix as a preview,
soak it briefly, and cut the next stable from `main`.

When `main` holds unfinished or risky work and a fix can't wait:

```
git switch -c release/0.4 v0.4.0          # once; reused for 0.4.2, 0.4.3 …
git cherry-pick -x <fix commit from main>
# bump Cargo to 0.4.1, CHANGELOG, PR into release/0.4, check passes
git tag -a v0.4.1 -m "…"  &&  git push origin release/0.4 v0.4.1
```

Delete `release/0.4` once 0.5.0 is out and 0.4 is no longer supported.

---

## 6. Cutting a release

### Preview

1. `main` is green.
2. `git tag -a v0.4.0-preview.3 -m "<what's new, user-facing, a few lines>"`
3. `git push origin v0.4.0-preview.3`
4. CI builds, packages, smoke-tests, publishes (§7). Update your own machine:
   `imperatorium update --channel preview`.

### Stable

1. The candidate preview has soaked (§4).
2. On a branch: Cargo version → `0.4.0`, `cargo update -p imperatorium`
   (lock file), move `CHANGELOG.md`'s *Unreleased* section under `0.4.0`.
   PR → `main`, squash-merge.
3. `git tag -a v0.4.0` on that merge commit, with the changelog section as the
   annotation. Push the tag.
4. CI publishes it as GitHub's "latest". Stable users' windows show the
   update badge on their next daily check.
5. Next PR: Cargo → `0.5.0-dev`.
6. Watch issues for 48 hours. A regression gets a `0.4.1` (§5), not a re-tag.

### Pulling a bad release

Mark the GitHub release as a pre-release (stable users stop seeing it as
"latest" and fall back to the previous stable), say so in its notes, and ship
a fixed patch. Never delete the release or its tag; installed binaries may be
mid-download.

---

## 7. What the release workflow guarantees

`release.yml` already builds every target from a tag, after `check`. To make
it the only door to users, it should also:

| Guarantee | How |
|---|---|
| The tag is a real release | Tag matches `^v\d+\.\d+\.\d+(-preview\.\d+)?$`, its commit is on `main` or `release/*`, and its version matches `Cargo.toml` (§2). |
| It installs before anyone else installs it | In `publish`, before `gh release create`: serve `dist/` on localhost and run `install.sh` (Linux runner) and `install.ps1` (Windows runner) against it with `IMPERATORIUM_RELEASES`, then `imperatorium install --yes` under a throwaway `--task-prefix/--state/--settings`, `imperatorium status`, `imperatorium update --check`. |
| The preview channel never 404s | Don't delete and recreate the `preview` release (there's a window where its URLs are gone). Create it once; each preview `gh release upload preview dist/* --clobber` and edits its title and notes. |
| Builds are provenanced | `actions/attest-build-provenance` on every asset; anyone can then `gh attestation verify imperatorium.exe -R hypertetrahedron/imperatorium`. |
| The manifest is authentic, not just intact | Sign `manifest.json` (minisign / Ed25519; private key in a GitHub secret). Embed the public key in the binary; `imperatorium update` refuses an unsigned or mis-signed manifest. Today's SHA-256 proves the binary matches the manifest, but both come from the same place. |
| Pinned toolchain and actions | Already: Rust 1.100.0, `--locked`. Also pin third-party actions by commit SHA (`humbletim/install-vulkan-sdk`, `Swatinem/rust-cache`). |
| The workflow can't be bypassed | Tag ruleset (§3); release environment with only `v*` tags allowed to deploy to it. |

### The release signing key

Made 2026-11-15 with `imperatorium release-keygen`. The public half is in
`RELEASE_KEYS` (`rust/imperatorium/src/signing.rs`); the secret is the
repository secret `IMPERATORIUM_SIGNING_KEY`, and the one other copy is
`%USERPROFILE%\.imperatorium-release\signing-key.json` on the maintainer's
machine (readable by that user only). That copy should move to offline
storage (a password manager or an encrypted USB key) and be deleted from
disk. If the secret is lost, releases can't be signed with it again; if it
leaks, anyone can sign a release. Either way: make a new pair, put the new
public key **first** in `RELEASE_KEYS` and keep the old one for one stable
release (so installed binaries take the update that brings the new key),
then drop the old one. A leaked key also means saying so in the release
notes.

Local mock releases (`rust/release/build.ps1`) are signed with a throwaway
key; set `IMPERATORIUM_RELEASE_PUBKEY` to the public key it prints to test
against them.

macOS builds are produced but have never run on a Mac (`continue-on-error`).
Until one has been tested, release notes should call macOS **experimental**,
and `install.sh` should say so when it runs on Darwin.

---

## 8. Changelog and notes

- `CHANGELOG.md` at the root, [Keep a Changelog](https://keepachangelog.com/)
  form: an *Unreleased* section at the top that each user-visible PR adds a
  line to, under **Added / Changed / Fixed / Breaking / Security**.
- A stable's tag annotation is its changelog section. A preview's annotation
  is a few lines of what's new since the last preview.
- **Breaking** entries say what the user has to do, if anything ("run
  `imperatorium install` again to update the hooks").
- Each release states the minimum Claude Code version it needs.

---

## 9. Installs and updates

### Keep the one-liners

They are the right primary method for a tool like this (rustup, uv, Bun,
Deno and Claude Code itself install the same way), and they already do the
important things: per-user, no admin, the binary checked against the
release's SHA-256 before it's put in place, and the real work handed to
`imperatorium install`, which can be dry-run.

```powershell
irm https://github.com/hypertetrahedron/imperatorium/releases/latest/download/install.ps1 | iex
```
```cmd
curl -fsSLo install.cmd https://github.com/hypertetrahedron/imperatorium/releases/latest/download/install.cmd && install.cmd
```
```sh
curl -fsSL https://github.com/hypertetrahedron/imperatorium/releases/latest/download/install.sh | sh
```

These URLs never change from release to release, which is what lets the
README show one command forever. If a shorter URL is ever wanted
(`imperatorium.dev/install.ps1`), make it a redirect to these, so the release
stays the single source.

### Harden them

1. **Truncation-safe scripts.** A pipe that drops mid-download runs whatever
   arrived. Wrap `install.sh`'s body in `main() { … }` and call `main "$@"`
   on the last line; same for `install.ps1` (a function invoked at the end).
2. **Version pinning.** `IMPERATORIUM_VERSION=0.4.0` →
   `releases/download/v0.4.0`, alongside the existing `IMPERATORIUM_CHANNEL`.
   For reproducible installs, CI, and "go back to the version that worked".
3. **Say what's about to happen** before `imperatorium install` changes
   anything — it already asks its questions; keep `--dry-run` prominent in the
   README for people who read before they pipe.
4. **Document the inspect-first path** next to each one-liner: download the
   script, read it, run it.

### Code signing

Unsigned Windows binaries that install scheduled tasks and replace themselves
are exactly what antivirus heuristics flag, and corporate machines (one of the
current users) may block them. So both Windows binaries, `imperatorium.exe`
and `imperatorium-voice.exe`, are Authenticode-signed with **Azure Artifact
Signing** (formerly Trusted Signing), set up 2026-11-16:

| | |
|---|---|
| Account | `Imperatorium` (resource group `Imperatorium`, East US, endpoint `https://eus.codesigning.azure.net/`) |
| Certificate profile | `ImperatoriumPublic` (Public Trust, tied to the maintainer's identity validation) |
| Subject | `CN=Lester Wilhelm, O=Lester Wilhelm, L=Sandy Springs, S=ga, C=US` |
| Timestamp | `http://timestamp.acs.microsoft.com` (SHA-256), so signatures outlive the short-lived certificates |

**In releases**, `release.yml`'s `windows` job signs both files right after
building them and before they are uploaded, so `release-package` records the
signed files' SHA-256 in the manifest. It then checks each one: status
`Valid`, the subject above, a timestamp. `smoke-windows` checks the exe the
installer actually downloaded the same way. Azure trusts the job through
**OIDC** (`azure/login`, then `azure/artifact-signing-action`, both pinned by
SHA): no secret is stored. The federated credential is for the `release`
environment, so only a run in that environment can sign.

A **stable** tag fails the build if signing is not set up; a **preview** is
signed when it is, and otherwise publishes unsigned with a warning.

**On this machine**, `rust\release\sign.ps1 [files]` signs through the
maintainer's script (`C:\Tools\signing\sign.ps1`, or the path in
`IMPERATORIUM_SIGN_SCRIPT`), which authenticates with the Azure CLI's
sign-in, then checks each file as the workflow does. With no files it signs
`rust\target\release\imperatorium.exe`; `-VerifyOnly` only checks.
`build.ps1 -Sign` signs before packaging. Sign a build before the live deploy
copies it in, and before anything records its hash. If signing fails with an
authentication error, sign in again:

    & "C:\Program Files\Microsoft SDKs\Azure\CLI2\wbin\az.cmd" login --tenant 18b17d67-bd7b-45bc-abb0-5cc440040688

**One-time setup for the workflow** (an Entra app with no secret, trusted
only for this repository's `release` environment, allowed only to sign with
this account; then three repository variables):

```powershell
$az  = "C:\Program Files\Microsoft SDKs\Azure\CLI2\wbin\az.cmd"
$sub = "28fe4e0d-7e64-413e-b2db-6a78ff32359a"
$acct = "/subscriptions/$sub/resourceGroups/Imperatorium/providers/Microsoft.CodeSigning/codeSigningAccounts/Imperatorium"
$app = & $az ad app create --display-name "imperatorium-release-signing" --query appId -o tsv
& $az ad sp create --id $app | Out-Null
@{ name = "release-environment"; issuer = "https://token.actions.githubusercontent.com"
   subject = "repo:hypertetrahedron/imperatorium:environment:release"
   audiences = @("api://AzureADTokenExchange") } | ConvertTo-Json | Set-Content fic.json
& $az ad app federated-credential create --id $app --parameters fic.json
& $az role assignment create --assignee $app --role "Artifact Signing Certificate Profile Signer" `
    --scope "$acct/certificateProfiles/ImperatoriumPublic"
# The release environment, deployable from v* tags only:
gh api -X PUT repos/hypertetrahedron/imperatorium/environments/release `
    -F "deployment_branch_policy[protected_branches]=false" -F "deployment_branch_policy[custom_branch_policies]=true"
gh api -X POST repos/hypertetrahedron/imperatorium/environments/release/deployment-branch-policies -f name="v*" -f type=tag
gh variable set AZURE_CLIENT_ID --body $app
gh variable set AZURE_TENANT_ID --body "18b17d67-bd7b-45bc-abb0-5cc440040688"
gh variable set AZURE_SUBSCRIPTION_ID --body $sub
```

(The role name was checked against this subscription on 2026-11-16.) The app can do
nothing but sign with this profile, and only a workflow job running in the
`release` environment (v* tags) can get a token for it.

macOS signing and notarisation (Apple Developer, $99/yr) can wait until
macOS is supported.

### Updates

`imperatorium update` and the window's update panel are the update path.
Rules for it:

- **Never update without consent.** Check daily (as now), show the badge;
  the person clicks Update. A setting may opt in to automatic stable patch
  updates later — never minors, never previews.
- **Keep one previous binary** (today's rename-aside does this but piles up
  `.old-*` files) and add `imperatorium update --rollback` to swap it back
  and restart the service. Prune the rest.
- **`imperatorium update --version 0.4.0`** installs a specific release, up or
  down.
- **State is versioned.** Add a `state_version` to the state folder. A newer
  binary migrates forward after copying the folder to a dated backup (as the
  2026-11-14 migration did by hand); an older binary that finds a newer
  `state_version` refuses to start with a message saying which version to
  install, rather than misreading the files. That's what makes `--rollback`
  safe.
- **Remote hosts follow the dispatcher.** `update` refreshes the hosts'
  builds, and before every channel start `Deploy::ensure` compares the host
  binary's SHA-256 with this build's and pushes it when they differ, so a
  host always runs byte-for-byte the dispatcher's version. No separate
  version handshake is needed.
- **Package-managed installs don't self-update.** If winget or Homebrew are
  added later, `update` detects the managed path and says "use winget
  upgrade" instead.

### Package managers: later, secondary

Worth adding once stable releases are regular, as conveniences rather than
replacements: a **winget** manifest (submitted by `wingetcreate` from
`release.yml`) and a **Homebrew tap** once macOS is supported. Both pin to
releases and SHA-256s produced by the same workflow, so they add no new trust.
Skip `cargo install` as an advertised path: it builds without the release's
static runtime and voice worker, and bypasses the manifest.

### Uninstall

One command, as the README says: `imperatorium uninstall` (hooks, mod,
service; config and state stay). Add `--purge` to remove config and state
too, so leaving is as easy as arriving.

---

## 10. Checklist summary

CI on GitHub first ran when `main` moved to the native build (2026-11-15)
and needed these to pass there, all in PR #1: Linux builds of the test
helpers, the runner's 8.3 short TEMP path (a real bug in worktree lookups
too), the goldens' time zone (US Eastern, set on the runners), gh's wording
under Actions, Windows-only tests marked so, and two test threads on the
4-core Windows runner. `remote add` from a Linux machine has no test yet.


Before the first public release (0.4.0-preview.1):

- [x] `main` fast-forwarded to `rust`; live worktree on `main`; merged branches deleted (2026-11-15)
- [x] Rulesets: `main` (PR + `check` on both runners), tags `v*` (create admin-only; never moved or deleted) (2026-11-15)
- [x] Cargo version `0.4.0-dev`; `release.yml` tag/branch/version checks
- [x] Rolling `preview` release updated in place, not recreated *(written; first runs on the first preview tag)*
- [x] Installer smoke tests before `publish` *(written; first runs on the first preview tag)*
- [x] `install.sh` / `install.ps1` truncation-safe; `IMPERATORIUM_VERSION` (install.ps1 tested piped and as a file on PowerShell 5.1 and 7)
- [x] `CHANGELOG.md`
- [x] README on `main` shows the native install

Before the first stable (0.4.0):

- [x] Signed manifest, verified by `update` (key made 2026-11-15; see the key section above)
- [x] `state_version` and forward migrations with backup (checked by `serve --live`; no migrations yet)
- [x] `update --rollback` and `update --version` (rollback restores the main binary; the voice worker stays as it is)
- [x] Build provenance attestations; actions pinned by SHA
- [ ] Windows code signing - signing works on this machine and `release.yml` signs with it (2026-11-16, Azure Artifact Signing; see Code signing); Azure app, federated credential, role, `release` environment and variables set up 2026-11-16; left: a preview tag to see it sign on the runner
- [x] macOS marked experimental in notes and installer
