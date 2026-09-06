// Heuristic detection of what an agent is doing, from the last screenful of its pane.
// Statuses: starting | working | idle | attention | exited

const ANSI = /\x1b\[[0-9;?]*[ -/]*[@-~]/g;

export function stripAnsi(s) {
  return s.replace(ANSI, '');
}

const EXITED = /\[agora\] agent "[^"]+" exited/;
const DOCUMENTING = /\[agora\] documenting session/;
const WORKING = /(esc to interrupt|esc to cancel|ctrl\+c to (cancel|interrupt|stop)|Thinking\.\.\.|Working\.\.\.)/i;
const ATTENTION =
  /(Do you want to|Allow (this|execution|once|always)|Would you like to|\(y\/n\)|\[Y\/n\]|\[y\/N\]|Yes, (allow|proceed|and don't ask|I trust)|Yes, run|approve this|Enter to confirm|Press Enter to continue|trust this folder|Select (an option|login method)|Paste (your|the) (code|key)|Sign in|Log ?in with|No, exit)/i;
// Prompt line for Claude Code (❯ / >), Codex (›), Gemini (>), possibly inside a box border (│).
const PROMPT_LINE = /(^|\n)[\s│┃|]*[>❯›]\s/;
const PROMPT_HINT = /(\? for shortcuts|Type your message|shift\+tab to cycle|Ctrl\+C to quit|for commands)/i;

export function detectStatus(rawText) {
  const text = stripAnsi(rawText);
  const lines = text.split('\n').filter((l) => l.trim());
  const tail = lines.slice(-16).join('\n');
  if (!tail) return 'starting';
  if (EXITED.test(tail)) return 'exited';
  if (DOCUMENTING.test(tail)) return 'documenting';
  // Permission / trust / login dialogs first: they often also carry "esc to cancel" hints.
  if (ATTENTION.test(tail)) return 'attention';
  if (WORKING.test(tail)) return 'working';
  if (PROMPT_LINE.test(tail) || PROMPT_HINT.test(tail)) return 'idle';
  return 'working';
}
