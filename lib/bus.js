// The delivery loop: reconciles agent rows with tmux windows, detects status,
// and pushes unread messages into idle agents' prompts.
import * as db from './db.js';
import * as tmux from './tmux.js';
import { detectStatus, stripAnsi } from './status.js';

const IDLE_GRACE_MS = 1500; // an agent must sit idle this long before we type into it
const MAX_NUDGE_CHARS = 1800;

// A nudge that is still sitting unsubmitted in the prompt box (the Enter got swallowed).
const UNSENT_NUDGE = /(^|\n)[\s│┃|]*[>❯›]\s*\[qoral\] /;

export function createBus() {
  const idleSince = new Map();
  const lastStatus = new Map();
  const awaitingSubmit = new Map(); // name -> { at, tries }

  function tick() {
    if (!tmux.serverUp()) return;
    const windows = new Set(tmux.listWindows().map((w) => w.id));
    const now = Date.now();

    for (const a of db.activeAgents()) {
      if (a.harness === 'moderator') continue; // virtual participant, no window
      if (!a.window_id || !windows.has(a.window_id)) {
        db.markExited(a.name);
        idleSince.delete(a.name);
        awaitingSubmit.delete(a.name);
        continue;
      }
      const screen = tmux.capture(a.window_id, 30);

      // Retry Enter if the last nudge is still sitting in the prompt box.
      const pendingSubmit = awaitingSubmit.get(a.name);
      if (pendingSubmit && now - pendingSubmit.at > 1200) {
        if (UNSENT_NUDGE.test(stripAnsi(screen)) && pendingSubmit.tries < 3) {
          tmux.pressEnter(a.window_id);
          awaitingSubmit.set(a.name, { at: now, tries: pendingSubmit.tries + 1 });
          continue;
        }
        awaitingSubmit.delete(a.name);
      }

      const status = detectStatus(screen);
      if (status !== lastStatus.get(a.name)) {
        lastStatus.set(a.name, status);
        if (status !== a.status) db.setStatus(a.name, status);
      }
      if (status === 'exited') {
        db.markExited(a.name);
        continue;
      }
      if (status !== 'idle') {
        idleSince.delete(a.name);
        continue;
      }
      if (!idleSince.has(a.name)) idleSince.set(a.name, now);
      if (now - idleSince.get(a.name) < IDLE_GRACE_MS) continue;

      const pending = db.pendingNudges(a.name);
      if (pending.length) {
        nudge(a, pending);
        idleSince.set(a.name, now); // back off before the next one
        awaitingSubmit.set(a.name, { at: now, tries: 0 });
      }
    }
  }

  return { tick };
}

function flatten(s) {
  return s.replace(/\s*\n+\s*/g, ' ⏎ ').replace(/\s+/g, ' ').trim();
}

export function formatNudge(agent, msgs) {
  let text;
  if (msgs.length === 1) {
    const m = msgs[0];
    text = `[qoral] message from ${m.sender}: ${flatten(m.body)}`;
  } else {
    text =
      `[qoral] ${msgs.length} new messages: ` +
      msgs.map((m, i) => `(${i + 1}) from ${m.sender}: ${flatten(m.body)}`).join('  ');
  }
  let truncated = false;
  if (text.length > MAX_NUDGE_CHARS) {
    text = text.slice(0, MAX_NUDGE_CHARS - 1) + '…';
    truncated = true;
  }
  const senders = [...new Set(msgs.map((m) => m.sender))];
  const replyHint =
    senders.length === 1
      ? `Reply with qoral_send(to="${senders[0]}").`
      : `Reply to each sender with qoral_send.`;
  text += truncated ? ` [truncated; call qoral_inbox for the full text.] ${replyHint}` : ` ${replyHint}`;
  return text;
}

function nudge(agent, msgs) {
  const text = formatNudge(agent, msgs);
  try {
    tmux.typeInto(agent.window_id, text);
    db.markNudged(msgs.map((m) => m.id));
  } catch {
    // window vanished mid-flight; the next tick will reconcile
  }
}
