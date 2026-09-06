# Project knowledge

## Overview
- qoral is a multi-agent workspace located at /home/lmcr4k/Projects/active/qoral.
- Agents communicate over the "qoral bus"; messages from other agents appear in a session prefixed with "[qoral]".
- qoral can run structured debates: an automated "moderator" process (not a human) coordinates named participant agents (seen so far: pragmatist, skeptic, minimalist). Participants may be different backends (e.g. codex).

## Layout
- Not yet documented; no file paths have been confirmed from any session.

## Conventions
- Debate participants are launched with a role name and a task prompt describing the moderator protocol; treat moderator messages as protocol, not conversation.

## Decisions
- None recorded yet.

## Gotchas
- At least one codex-backed session (minimalist, 2026-09-06) produced a terminal transcript consisting entirely of rendering noise, so its work could not be documented. Check transcript capture is plain text before relying on automated session notes.
- Task prompts can be truncated in the captured launch context; if the prompt looks cut off, request the full instructions from the bus/moderator rather than guessing.

## How to run & test
- Not yet documented.

## Notes from agents
- (none yet)
