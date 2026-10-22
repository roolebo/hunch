# tribe: Human-centric convention for GenAI

Instead of building workflows around an AI harness build it around the human,
to augment their intellect [1]. Keep the proven ideas: Unix composition, KISS,
CSP [2], text as the interface, small tools. LLMs also excel in the same lean
environment.

Version 0.1.0. tribe, its registry and protocols, is a concept still being
worked out as participants join, so it isn't ideal and will change. Until it
settles the version stays 0.x: a breaking change bumps the minor version (0.1
to 0.2), a compatible one the patch. 1.0 comes once it works well for several
editors and agents. A participant's plugin states which version it implements.

## Why

Engelbart named the goal: augmenting human intellect, "increasing the
capability of a man to approach a complex problem situation" [1]. He saw the
human as part of a system: a Human using Language, Artifacts, Methodology, in
which he is Trained (H-LAM/T). The system improves as a whole, never by one
artifact alone.

tribe grew out of practice and came to the same view. An agent is a new
artifact. Put inside a new editor or a chat window, it asks the human to drop
their training: their editor, their keys, their way of working. tribe adds
agents to the system the human already has:

- Human: stays the center. Agents and tools work around what the human works
  on.
- Language: plain text protocols that a person can read and a shell can run.
- Artifacts: the editor, the terminal, the agents. Each one can be replaced.
- Methodology: small edits happen live in the human's editor; larger changes
  come as a series of commits to review.
- Training: the human keeps their editor, keys and habits.

## Principles
- The center is whatever the human works on. People work best focused on one
  thing: switching between too many things breaks the natural flow.

- Nothing is fixed: models, harnesses, editors, project tools, and the AI
  tooling field itself keep changing. The aproach should be be able absorb
  whatever comes next without redesign.

- Input and output (text, voice, keystrokes, pipes, TCP, etc) are separate and
  interchangeable

## Model

- Participant: a process that takes part in the work: the human, an agent,
  a tool. Participants share no state. A participant has a role (what it is)
  and an identity unique within its environment.

- Channel: the only way participants communicate. A channel carries messages
  between two participants, as in CSP, but is exposed to the outside where it
  can be inspected and rewired.

- Address: a URI that says how to open a channel to a participant. Each address
  is valid within a scope: some only inside an environment, some also outside.
  New URI schemes add new ways to connect.

- Environment: a group of participants that can reach each other by addresses
  valid inside it. An environment is itself a participant, so environments
  connect through channels using addresses valid between them, and can nest.

- Registry: an environment may keep a registry where its participants register
  themselves: role, identity and addresses, for as long as they live.
  A registry is optional: without one, participants learn addresses some other
  way, e.g. from configuration or from messages.

- Routing: wiring channels between participants. Composing a workflow means
  routing: human to agent, agent to center, agent to tool, tool to tool,
  environment to environment.

## Editor protocol

Specified in [skills/tribe/PROTOCOL.md](skills/tribe/PROTOCOL.md).

## Agents

`tribe-editor` finds the editor in tribe and runs its handler.
`tribe-editor guard FILE` exits 2, with the reason on stderr, if FILE must
change through the editor protocol.

The repo comes with the `tribe` skill for agents.

## Integrations

- [vim-tribe]: Vim as an editor
- [tmux-tribe]: pastes in an editor go to the agent
- [claude-tribe]: Claude Code, guard and registration as an agent

[vim-tribe]: https://github.com/roolebo/vim-tribe
[tmux-tribe]: https://github.com/roolebo/tmux-tribe
[claude-tribe]: https://github.com/roolebo/claude-tribe

References:
1. Augmenting Human Intellect: A Conceptual Framework. https://www.dougengelbart.org/pubs/papers/scanned/Doug_Engelbart-AugmentingHumanIntellect.pdf
2. Communicating Sequential Processes. https://www.cs.cmu.edu/~crary/819-f09/Hoare78.pdf
