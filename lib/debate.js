// `qoral debate`: several agents argue a question through the bus under a deterministic
// moderator (this process), converge or time out, and a headless model writes the decision
// into <project>/.qoral/decisions/ and links it from KNOWLEDGE.md. Optionally a builder implements it.
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import * as db from './db.js';
import { spawnAgent, killAgent } from './agents.js';
import { HARNESSES } from './paths.js';
import { knowledgeDir, knowledgeFile, readKnowledge, runSummarizer, pickSummarizer } from './knowledge.js';

export const MODERATOR = 'moderator';

const PERSONAS = [
  {
    name: 'pragmatist',
    brief: 'You care about what ships and what the team can maintain: follow the project\'s existing conventions unless there is a strong reason not to, prefer boring proven approaches, and weigh implementation effort honestly.',
  },
  {
    name: 'skeptic',
    brief: 'You care about correctness, edge cases and failure modes. Attack every proposal for what breaks, what is untested, and what surprises callers. Hold your position for at least two rounds unless someone shows a concrete flaw in it.',
  },
  {
    name: 'minimalist',
    brief: 'You argue for the simplest thing that can work: fewest concepts, least code, no speculative generality. Push back on anything that adds a dependency, an abstraction or a configuration knob without a present need.',
  },
  {
    name: 'architect',
    brief: 'You take the long view: how the choice constrains future changes, performance at scale, API ergonomics for callers, and consistency with the wider codebase.',
  },
];

const which = (cmd) => spawnSync('sh', ['-c', `command -v ${cmd}`], { encoding: 'utf8' }).status === 0;

export function defaultDebaters(count = 3) {
  const preferred = ['claude', 'agy', 'codex', 'gemini'].filter((h) => HARNESSES.includes(h) && which(h));
  if (!preferred.length) throw new Error('no agent CLIs found on PATH');
  const out = [];
  for (let i = 0; i < count; i++) out.push(preferred[i % preferred.length]);
  return out;
}

function stamp(d = new Date()) {
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}`;
}
const slugify = (s) => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 48) || 'debate';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function parseReply(body) {
  const stances = [...body.matchAll(/STANCE:\s*(.+)/gi)];
  const cons = [...body.matchAll(/CONSENSUS:\s*(yes|no)/gi)];
  return {
    stance: stances.length ? stances[stances.length - 1][1].trim() : null,
    consensus: cons.length ? cons[cons.length - 1][1].toLowerCase() === 'yes' : false,
  };
}

function debaterBrief({ name, persona, question, cwd, participants, rounds }) {
  const others = participants.filter((p) => p !== name);
  return [
    `You are "${name}", one of ${participants.length} participants (${participants.join(', ')}) in a structured debate run by "${MODERATOR}". The moderator is an automated process on the qoral bus, not a person: it only reads messages sent to it and relays positions between participants.`,
    ``,
    `QUESTION: ${question}`,
    ``,
    `YOUR PERSPECTIVE: ${persona}`,
    ``,
    `Ground every claim in this project (${cwd}): read the relevant files first with your file-reading tools (avoid shell commands; they may block on a permission prompt nobody is watching). Do NOT edit, create or delete any files during the debate.`,
    ``,
    `PROTOCOL (${rounds} rounds max):`,
    `1. Investigate briefly, then send your opening position with qoral_send(to="${MODERATOR}"): at most 250 words, concrete, citing files where relevant. End the message with exactly these two lines:`,
    `   STANCE: <one-line summary of the approach you advocate>`,
    `   CONSENSUS: no`,
    `2. Immediately call qoral_wait(timeout_seconds=600). The moderator will send you the other participants' positions.`,
    `3. Each round: engage with the strongest points from ${others.join(' and ')}, say explicitly what changed your mind (if anything), and send your updated position to ${MODERATOR} ending with the same two lines. Write "CONSENSUS: yes" only when you fully accept ONE shared approach; then your STANCE line must describe that shared approach in the same words the others would use.`,
    `4. After each reply call qoral_wait(timeout_seconds=600) again. Stop when the moderator says the debate is over.`,
    ``,
    `Rules: send everything to ${MODERATOR}, never to the other participants or to human directly. One message per round. Be direct; concede when the argument against you is better, hold firm when it is not.`,
  ].join('\n');
}

function roundMessage({ round, rounds, positions, me }) {
  const others = Object.entries(positions).filter(([n]) => n !== me);
  const parts = [`Round ${round} of ${rounds}. Positions from the other participants:`];
  for (const [n, p] of others) parts.push(``, `--- ${n} ---`, p.body.trim());
  parts.push(
    ``,
    `Respond now: critique these, update your position if their argument is better, and reply to ${MODERATOR} ending with your STANCE and CONSENSUS lines. Then qoral_wait again.`,
  );
  return parts.join('\n');
}

function synthesisPrompt({ question, cwd, consensus, participants, finalStances }) {
  return `You are the moderator's scribe for a structured design debate between AI coding agents working in the project at ${cwd}.

QUESTION: ${question}

Participants: ${participants.join(', ')}.
Outcome: ${consensus ? 'the participants reached consensus.' : 'the participants did NOT fully converge within the round limit; you must pick the best-supported position and say why.'}
Final stances:
${Object.entries(finalStances).map(([n, s]) => `- ${n}: ${s || '(no explicit stance)'}`).join('\n')}

After this prompt you will receive the CURRENT PROJECT KNOWLEDGE and the FULL DEBATE TRANSCRIPT.

Write one markdown document and nothing else, with exactly these sections:
# Decision: <short title>
**Question:** one line.
**Decision:** the chosen approach in 1–3 sentences (label it "Recommendation" instead of "Decision" if there was no consensus).
## Rationale
The strongest arguments that carried, grounded in the project's files and conventions.
## Alternatives considered
Each rejected option and the concrete reason it lost.
## Dissent and risks
Remaining objections, edge cases to watch, and what would change the decision.
## Implementation plan
Numbered, concrete steps with file paths, tests to add, and how to verify.
## Participants
One line per participant: perspective and how their position moved.`;
}

function appendDecisionToKnowledge(cwd, line) {
  fs.mkdirSync(knowledgeDir(cwd), { recursive: true });
  let doc = readKnowledge(cwd);
  if (!doc) doc = `# Project knowledge\n\n## Decisions\n`;
  if (!/^## Decisions\s*$/m.test(doc)) doc = doc.trimEnd() + `\n\n## Decisions\n`;
  // insert right after the Decisions heading block (before the next heading)
  const idx = doc.search(/^## Decisions\s*$/m);
  const headEnd = doc.indexOf('\n', idx) + 1;
  const nextHeading = doc.slice(headEnd).search(/^## /m);
  const insertAt = nextHeading < 0 ? doc.length : headEnd + nextHeading;
  const before = doc.slice(0, insertAt).trimEnd();
  const after = doc.slice(insertAt);
  doc = `${before}\n${line}\n${after ? '\n' + after.replace(/^\n+/, '') : ''}`;
  fs.writeFileSync(knowledgeFile(cwd), doc.trimEnd() + '\n');
}

/**
 * Run a debate. Returns { decisionPath, consensus, rounds, finalStances, builder }.
 */
export async function runDebate({
  question,
  cwd,
  harnesses,
  rounds = 3,
  timeoutSec = 300,
  build = false,
  keep = false,
  log = console.log,
}) {
  question = String(question || '').trim();
  if (!question) throw new Error('a question is required');
  cwd = path.resolve(cwd || process.cwd());
  harnesses = harnesses?.length ? harnesses : defaultDebaters(3);
  if (harnesses.length < 2) throw new Error('a debate needs at least two participants');
  if (harnesses.length > PERSONAS.length) throw new Error(`at most ${PERSONAS.length} participants`);
  for (const h of harnesses) if (!HARNESSES.includes(h)) throw new Error(`unknown harness "${h}"`);

  db.open();
  const t0 = Date.now();
  const elapsed = () => `${Math.round((Date.now() - t0) / 1000)}s`;

  // moderator: a virtual agent row so participants can address it on the bus
  const prev = db.getAgent(MODERATOR);
  if (prev) db.deleteAgent(MODERATOR);
  db.insertAgent({ name: MODERATOR, harness: 'moderator', cwd, windowId: null, task: `debate: ${question.slice(0, 160)}` });
  db.setStatus(MODERATOR, 'moderating');

  const participants = harnesses.map((h, i) => {
    let name = PERSONAS[i].name;
    const taken = db.getAgent(name);
    if (taken && !taken.exited_at) name = `${name}-${(Date.now() % 1000).toString(36)}`;
    return { name, harness: h, persona: PERSONAS[i].brief };
  });
  const names = participants.map((p) => p.name);
  const spawned = [];
  const transcript = []; // { round, from, body }
  let lastId = db.recentMessages(1)[0]?.id ?? 0;

  const cleanup = () => {
    if (!keep) for (const n of spawned) try { killAgent(n, { document: false }); } catch {}
    try { db.deleteAgent(MODERATOR); } catch {}
  };
  const onSignal = () => { log(`\n[${elapsed()}] interrupted; cleaning up`); cleanup(); process.exit(130); };
  process.on('SIGINT', onSignal);
  process.on('SIGTERM', onSignal);

  try {
    log(`debate: ${question}`);
    log(`project: ${cwd}`);
    log(`participants: ${participants.map((p) => `${p.name} (${p.harness})`).join(', ')} · ${rounds} rounds max · ${timeoutSec}s per round`);
    for (const p of participants) {
      const a = spawnAgent({
        harness: p.harness,
        name: p.name,
        cwd,
        prompt: debaterBrief({ name: p.name, persona: p.persona, question, cwd, participants: names, rounds }),
        focus: false,
      });
      spawned.push(a.name);
      log(`[${elapsed()}] spawned ${a.name} (${a.harness})`);
    }

    // collect one reply per participant per round
    const warned = new Set();
    async function collect(round) {
      const got = {};
      const deadline = Date.now() + timeoutSec * 1000;
      while (Object.keys(got).length < names.length && Date.now() < deadline) {
        // A participant stuck on a permission/trust prompt can't reply; tell the human where to look.
        for (const n of names) {
          if (got[n] || warned.has(`${round}:${n}`)) continue;
          const a = db.getAgent(n);
          if (a?.status === 'attention') {
            warned.add(`${round}:${n}`);
            log(`[${elapsed()}] ${n} is waiting on a prompt in its window (permission/trust). Answer it in qoral to let it continue.`);
            db.addMessage(MODERATOR, 'human', `${n} is blocked on a prompt in its window during the debate. Select it in the sidebar and answer it.`);
          } else if (a?.exited_at) {
            warned.add(`${round}:${n}`);
            log(`[${elapsed()}] ${n} exited; continuing without them`);
          }
        }
        for (const m of db.messagesSince(lastId, 500)) {
          lastId = Math.max(lastId, m.id);
          if (m.recipient !== MODERATOR || !names.includes(m.sender)) continue;
          db.markRead([m.id]);
          const parsed = parseReply(m.body);
          // prefer a message that carries a STANCE line; otherwise keep the latest
          if (!got[m.sender] || parsed.stance || !got[m.sender].stance) got[m.sender] = { body: m.body, ...parsed };
          transcript.push({ round, from: m.sender, body: m.body });
          log(`[${elapsed()}] round ${round}: ${m.sender} → ${parsed.stance ? `STANCE: ${parsed.stance}` : '(no stance line)'}${parsed.consensus ? '  [consensus: yes]' : ''}`);
        }
        if (Object.keys(got).length < names.length) await sleep(1500);
      }
      const missing = names.filter((n) => !got[n]);
      if (missing.length) log(`[${elapsed()}] round ${round}: no reply from ${missing.join(', ')} within ${timeoutSec}s; continuing without them`);
      return got;
    }

    let positions = {};
    let consensus = false;
    let roundsRun = 0;
    for (let round = 1; round <= rounds; round++) {
      roundsRun = round;
      if (round > 1) {
        for (const n of names) {
          if (!positions[n] && round > 2) continue; // silent participant: stop poking
          db.addMessage(MODERATOR, n, roundMessage({ round, rounds, positions, me: n }));
        }
        log(`[${elapsed()}] round ${round}: positions relayed, waiting for replies…`);
      } else {
        log(`[${elapsed()}] round 1: waiting for opening positions…`);
      }
      const got = await collect(round);
      positions = { ...positions, ...got };
      const replied = Object.values(got);
      if (replied.length >= 2 && replied.length === names.length && replied.every((p) => p.consensus)) {
        consensus = true;
        log(`[${elapsed()}] consensus reached after ${round} round(s)`);
        break;
      }
      if (!replied.length) {
        log(`[${elapsed()}] nobody replied; ending debate`);
        break;
      }
    }

    for (const n of names) db.addMessage(MODERATOR, n, `The debate is over. Thank you. Do not send further messages; stop and wait.`);

    const finalStances = Object.fromEntries(names.map((n) => [n, positions[n]?.stance ?? null]));

    // synthesis
    fs.mkdirSync(path.join(knowledgeDir(cwd), 'decisions'), { recursive: true });
    const decisionPath = path.join(knowledgeDir(cwd), 'decisions', `${stamp()}-${slugify(question)}.md`);
    const header = `---\nquestion: ${JSON.stringify(question)}\nparticipants: ${names.join(', ')}\nharnesses: ${harnesses.join(', ')}\nrounds: ${roundsRun}\nconsensus: ${consensus}\ndate: ${new Date().toISOString()}\n---\n\n`;
    const debateText = transcript.map((t) => `### round ${t.round} — ${t.from}\n${t.body.trim()}`).join('\n\n');
    let body;
    const summarizer = pickSummarizer();
    if (summarizer) {
      log(`[${elapsed()}] writing decision document with ${summarizer}…`);
      try {
        const { output } = runSummarizer(
          synthesisPrompt({ question, cwd, consensus, participants: names, finalStances }),
          `CURRENT PROJECT KNOWLEDGE:\n${readKnowledge(cwd) || '(empty)'}\n\nFULL DEBATE TRANSCRIPT:\n${debateText}`,
          cwd,
          { summarizer },
        );
        body = output.trim().replace(/^```(?:markdown|md)?\s*\n?/, '').replace(/\n?```\s*$/, '');
      } catch (e) {
        log(`[${elapsed()}] synthesis failed (${e.message}); saving raw transcript instead`);
      }
    }
    if (!body) {
      body =
        `# Decision: ${question}\n\n**Question:** ${question}\n\n**Outcome:** ${consensus ? 'consensus' : 'no consensus'}\n\n## Final stances\n` +
        Object.entries(finalStances).map(([n, s]) => `- **${n}**: ${s || '(none)'}`).join('\n') +
        `\n\n## Transcript\n\n${debateText}\n`;
    }
    fs.writeFileSync(decisionPath, header + body.trim() + '\n\n---\n\n## Debate transcript\n\n' + debateText + '\n');
    const title = (body.match(/^#\s*Decision:\s*(.+)$/m)?.[1] ?? question).trim();
    const rel = path.relative(cwd, decisionPath);
    appendDecisionToKnowledge(cwd, `- (${new Date().toISOString().slice(0, 10)}, debate${consensus ? '' : ', no consensus'}) ${title} — see ${rel}`);
    log(`[${elapsed()}] decision: ${decisionPath}`);
    log(`[${elapsed()}] linked from ${path.relative(cwd, knowledgeFile(cwd))}`);

    db.addMessage(MODERATOR, 'human', `DECISION${consensus ? '' : ' (no consensus; recommendation)'}: ${title}. Full write-up: ${rel}`);

    // optional builder
    let builder = null;
    if (build) {
      const bh = harnesses.includes('claude') ? 'claude' : harnesses[0];
      const a = spawnAgent({
        harness: bh,
        name: db.getAgent('builder') && !db.getAgent('builder').exited_at ? `builder-${(Date.now() % 1000).toString(36)}` : 'builder',
        cwd,
        prompt:
          `Implement the decision recorded in ${rel} (read it first, including the implementation plan). ` +
          `Follow the project's conventions, add or update tests as the plan says, run the test suite, and when everything passes send the human a short report with qoral_send(to="human") listing the files you changed. ` +
          `If the plan turns out to be infeasible, stop and explain to human instead of improvising a different design.`,
        focus: true,
      });
      builder = a.name;
      log(`[${elapsed()}] builder ${a.name} (${a.harness}) started`);
    }

    if (!keep) {
      await sleep(4000); // let the closing message land
      for (const n of spawned) try { killAgent(n, { document: false }); } catch {}
      log(`[${elapsed()}] participants dismissed (use --keep to leave them running)`);
    } else {
      log(`[${elapsed()}] participants kept running: ${spawned.join(', ')}`);
    }
    db.deleteAgent(MODERATOR);
    process.off('SIGINT', onSignal);
    process.off('SIGTERM', onSignal);

    log('');
    log(body.trim());
    return { decisionPath, consensus, rounds: roundsRun, finalStances, builder };
  } catch (e) {
    cleanup();
    throw e;
  }
}
