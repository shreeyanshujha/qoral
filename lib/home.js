// The placeholder window shown in the agents pane when no agent is selected.
export async function runHome() {
  const B = '\x1b[1m', D = '\x1b[2m', Y = '\x1b[33m', R = '\x1b[0m';
  const lines = [
    '',
    `  ${B}${Y}agora${R}  ${D}many agents, one room${R}`,
    '',
    `  Nothing selected yet. Start an agent from the sidebar on the left:`,
    '',
    `    ${B}n${R}        new agent (pick claude / codex / gemini, a name, a directory, a task)`,
    `    ${B}Enter${R}    show the selected agent here`,
    `    ${B}m${R}        message the selected agent      ${B}b${R}  broadcast to all`,
    `    ${B}l${R}        open the message log            ${B}?${R}  full help`,
    '',
    `  From anywhere:`,
    '',
    `    ${B}Alt+←/→${R}  move between sidebar and agent   ${B}Alt+n${R}  new agent`,
    `    ${B}Alt+[ ]${R}  previous / next agent            ${B}Alt+1..9${R}  jump to agent N`,
    `    ${B}Alt+d${R}    detach (agents keep running; run ${B}agora${R} to come back)`,
    '',
    `  Agents can talk to each other through the ${B}agora${R} MCP tools`,
    `  (agora_send, agora_wait, agora_spawn, ...). Messages to an idle agent are`,
    `  typed straight into its prompt, so conversations flow without polling.`,
    '',
    `  ${D}CLI: agora spawn claude --dir ~/proj "task"   agora send ada "hi"   agora list   agora log${R}`,
    '',
  ];
  process.stdout.write('\x1b[2J\x1b[H' + lines.join('\n') + '\n');
  process.stdout.on('resize', () => process.stdout.write('\x1b[2J\x1b[H' + lines.join('\n') + '\n'));
  process.on('SIGTERM', () => process.exit(0));
  process.on('SIGINT', () => {});
  // A pending promise alone does not keep the event loop alive; a timer does.
  setInterval(() => {}, 1 << 30);
  await new Promise(() => {});
}
