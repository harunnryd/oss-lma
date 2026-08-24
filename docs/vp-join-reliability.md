---
title: "VP Join Reliability"
---

# VP Join Reliability

**Joins fail for mundane reasons**: the platform moved a button, the meeting
sits behind a waiting room or a verification wall, the passcode rotated, or
the network blipped mid-navigation. A headless browser cannot improvise the
way a human does, so the [Virtual Participant](virtual-participant.md)
attacks each failure mode with a dedicated mechanism: hardcoded selectors
backed by an **AI DOM resolver**, a **persistent Chromium profile** that
makes signed-in joins stick, a **dialog watchdog** that auto-dismisses
consent prompts and escalates walls it cannot clear, and the **manual
takeover view** for anything that genuinely requires a human. The design
rule throughout: never report success silently — a join that did not happen
fails loudly instead of transcribing an empty room.

## Failure taxonomy

| Failure mode | Detected by | Automatic response | User-facing surface |
|---|---|---|---|
| Network blip mid-join | navigation timeout, or the sidecar WebSocket drops | audio reconnects on its own (0.5–10 s backoff, ≤3 s buffer, fresh `START` with the same `CallId`); navigation retried within the attempt budget | task stays `JOINING` with a live substep label; only unrecoverable drops surface as `VP_CONTAINER_FAILED` |
| Wrong meeting ID / passcode | prejoin controls absent (bad ID), or submission rejected after the verify-and-retype pass | none — deterministic failures fail fast instead of retrying into a platform lockout | task `FAILED` with reason; fix the schedule, then Retry Now |
| Waiting room | admission poll finds no in-meeting signal | keep polling up to 5 min; +5 min extension while an escalation is active | live status "Waiting to be admitted… (host may need to admit the participant)" |
| CAPTCHA | dialog watchdog classifies the modal as `CAPTCHA`/`BLOCKED`; sign-in driver reports a manual-required outcome | sign-in page reloaded so the human gets a clean form, then escalate to `AWAITING_ACTION` | takeover banner with category-specific guidance; 300 s window |
| 2FA / SSO wall | same classifier — `otp-2fa`, `sso`, `LOGIN_REQUIRED` reasons | escalate to `AWAITING_ACTION`; success confirmed by polling for real session cookies, not by page URL alone | takeover banner naming the wall type; join resumes automatically once cleared |
| Platform DOM change | element-wait timeouts exhaust primary selectors for a step | AI DOM resolver re-derives and caches selectors (≤2 attempts) | task log line `Resolved <step> via cache/ai`; unresolved steps escalate |
| Browser crash | renderer crash event or container exit | pre-join: one automatic container restart per task; in-meeting: meeting finalizes with whatever was captured | `VP_CONTAINER_FAILED` toast; task log shows the crash |
| Host-admission required ("Sign in to join this meeting") | auth prompt text on the meeting page | escalate to `AWAITING_ACTION` with a shorter 120 s window; a stored signed-in account prevents recurrence | takeover banner; subsequent joins skip the wall entirely |

## Credentials and profiles

Two stores, deliberately split:

| Store | Holds | Never holds |
|---|---|---|
| **Profile volume** (named Docker volume, survives restarts) | cookies, localStorage, trusted-device markers, browsing state — per platform (`zoom`, `meet` are separate volumes) | any password or API key |
| **OS keychain** | platform username/password for optional signed-in joins, referenced by a settings row | browser profile data |

The database and config files hold references only; the plaintext is never
returned to the UI (status shows present / last-updated), never baked into
the image, and never passed through task metadata.

**Guest vs signed-in.** Guest joins work on open meetings and need nothing
stored. A stored account signs in before navigating, which avoids the
"we detected you may be a bot — sign in to join" guest block, unlocks
meetings that disallow anonymous participants, and rides on the trusted
cookies the profile already holds. Two account-hygiene rules carry over
from production experience: brand-new accounts join less reliably (sign in
once from your normal browser first), and being signed in to the same
account from several places at once invites repeat CAPTCHAs — sign out
elsewhere before blaming the bot.

**Session survival.** Chromium keeps session cookies only in memory, which
would log a restored profile out on every launch. On restore, the container
promotes meeting-platform session cookies to persistent with a ~30-day
lifetime, so a sign-in completed once — including one cleared by hand in
the takeover view — survives container restarts. Platforms still expire
sessions on their own schedule; an expired session presents as an ordinary
sign-in wall, one `AWAITING_ACTION` clears it, and the refreshed profile
carries forward.

Removing stored credentials resets that platform's profile, so cached
cookies can never outlive the credentials they came from.

## AI DOM resolver

Every adapter step waits on its primary CSS selectors with bounded retries
(10 tries × 500 ms ≈ 5 s) before declaring the step broken — most misses
are slow renders, not breakage.

When the primaries exhaust:

1. The resolver receives the step's **intent in plain language** ("the
   Zoom prejoin Join button that submits the form"), a snapshot of the
   page's interactive elements (tag, id, role, aria-label, visible text,
   bounding boxes), and a PNG screenshot.
2. The configured LLM returns a candidate selector, which is accepted only
   if it matches exactly one visible element in the live DOM.
3. The rejected attempt plus its failure reason accompany the second ask,
   so attempt 2 does not repeat attempt 1's mistake.
4. After **2 failed resolver attempts** the step escalates: interactive
   stages go to `AWAITING_ACTION`; best-effort stages degrade gracefully
   (a chat hiccup never fails a joined meeting).

Caching and invalidation:

- Successful resolutions are cached under a `platform#step` key inside the
  container volume, with a 30-day sliding TTL refreshed by every verified
  hit — the first meeting after a platform UI change pays the LLM cost;
  every later meeting hits the cache.
- A cached selector that stops matching the live DOM is evicted
  automatically and re-resolved in the same join.
- `RESET_SELECTORS` (dashboard → task → reset selectors, delivered as a
  `VP_COMMAND`) wipes the platform's entries to force full re-derivation —
  see [Virtual Participant Local
  Development](virtual-participant-local-dev.md#selector-cache).

Two watchdogs run alongside it:

- **Dialog watchdog** — scans for modals every 5 s (backing off to 20 s on
  a quiet page). An unknown modal stable across two consecutive scans is
  classified from vision + DOM: consent and recording notices are clicked
  through automatically; CAPTCHA/SSO/login/blocked walls escalate. The
  escalation banner self-clears when the dialog disappears. Unclassifiable
  dialogs are left alone — nothing is auto-clicked blindly.
- **Join-state classifier** — during the admission wait, a periodic
  second opinion (every 30 s) asks the model what screen the browser is
  really on, so a renamed in-meeting class name can never strand the task
  at "Waiting" until timeout, and an error screen ends the wait early
  instead of burning the full budget.

## Retry policy

Budgets inside one join attempt:

| Budget | Value |
|---|---|
| Prejoin settle wait (URL stable + controls visible) | 30 s cap |
| Passcode entry | type → verify value landed → retype once |
| Join submit | click → check progress after 6 s → resubmit once |
| Admission wait | poll every 1.5 s for 5 min (+5 min while escalated) |
| Resolver attempts per step | 2, then escalate |
| Human action window once escalated | 300 s, then FAILED |
| Auth-required prompt window | 120 s, then FAILED |

Across attempts:

- One scheduled occurrence = one task = one container. `VP_CONTAINER_FAILED`
  grants a single automatic container restart; anything beyond that is
  final for the occurrence.
- **FAILED is terminal.** There is no automatic retry with backoff at the
  occurrence level — retrying a wrong passcode or expired credentials just
  hammers the platform and risks lockout, so deterministic failures stop
  immediately.
- For recurring schedules the RRULE *is* the retry mechanism: the next
  firing starts a fresh task with fresh state. One-off meetings get Retry
  Now in the dashboard, which spawns a new task rather than resurrecting
  the failed one.
- Escalations are not failures yet: an `AWAITING_ACTION` task that times
  out marks FAILED but keeps its banner and reason visible, so the fix
  (complete the sign-in, reset selectors, re-save credentials) is one
  glance away — see [Troubleshooting](troubleshooting.md).

## Manual takeover playbook

The takeover view ([Desktop App Guide](desktop-app-ui.md)) streams live
screenshots of the bot's display; clicks and keystrokes pass through as
`CLICK`/`TYPE` commands, and `end` / `pause` / `start` chat commands plus
selector-cache reset sit beside the stream.

1. Open the dashboard → Virtual Participant → the task shows an
   `AWAITING_ACTION` banner naming the wall category. Click **Takeover**.
2. Clear the wall using the matching procedure below. You do not need to
   restart anything afterwards — the join continues from where it stopped.
3. The banner clears itself once the blocking dialog disappears; watch the
   screenshot stream advance through prejoin → admission → in-meeting.

Per wall:

- **Waiting room** — nobody has to touch the bot at all: admit the
  participant from your own client, or ask the host to. The takeover view
  just shows the "asking to be let in" screen while the admission poll
  runs.
- **CAPTCHA** — solve it directly in the takeover view. If it recurs every
  attempt, sign the account out elsewhere first (another device, or the
  platform's web site in your own browser); simultaneous sessions are the
  usual trigger. The container reloads the sign-in page before escalating
  so you always get a clean form, not the half-interacted one.
- **2FA** — approve the push notification or type the one-time code in the
  takeover view. Completion is verified against real session cookies
  server-side, so a half-finished sign-in is never mistaken for success.
- **SSO** — complete the corporate login flow in the view. This is the one
  wall that may recur per password rotation; each clear refreshes the
  profile.

Afterwards the persistent profile keeps the trust: subsequent joins for
that platform usually walk straight in. A wall that keeps coming back means
stale state — reset the selector cache and profile from the task menu,
re-save the credentials, and check the task log before the next occurrence.
