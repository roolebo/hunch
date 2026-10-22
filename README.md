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

## Example: voice in Vim

The human talks to an agent without leaving Vim and without new keys. The
setup:

- iTerm2 runs tmux with two panes: Vim and Claude Code.
- Vim is started with --servername; [vim-tribe] registers it as the editor.
- [claude-tribe] registers Claude Code as an agent.
- [tmux-tribe] is loaded in tmux.
- Pipit, a dictation app, pastes speech into the terminal while a key is held.

The human selects a function in Vim, holds the key and says "split this in
two". Vim is in normal mode, so tmux-tribe sends the text to the agent instead
of the editor. The agent asks the editor what is selected, makes the change
through the editor without moving the human's view, or prepares commits and
opens them in Vim for review. In insert mode the same dictation types into
the buffer, as before.

The terminal only carries the paste, so this works over SSH as well: tmux,
Vim and the agent run on a remote machine, Pipit and iTerm2 on the local one.
The cursor and the selection say what "this" and "here" mean.

## Why not

ACP, the Agent Client Protocol [3]: the editor starts the agent as its child
and shows it in its own panel. The editor becomes the center instead of the
human's work, and the agent lives only as long as that editor. The human has
to use an editor that implements ACP and talk to the agent through its panel.
Only two participants exist: one editor and its agent. Other agents, tools
and environments have no place in it.

MCP, the Model Context Protocol [4]: the AI harness is the center and
everything else is its tool. Editor integrations of agents, such as Claude
Code's, are built on it: the editor runs a server for one vendor's agent,
found through that vendor's lock files. Each editor implements each vendor's
protocol, which is the coupling tribe avoids. It serves the model, not the
human.

AI editors such as Cursor: the human gives up their editor and habits for the
vendor's. Models, harnesses and editors keep changing, and each change means
moving again.

Agents inside an editor, as plugins: each editor rebuilds the agent, and the
human can't use an agent from elsewhere. The agent stops with the editor.

Plain tmux, sending keys to a pane: text without structure. The agent can't
ask where the cursor is or make an edit that keeps the human's view.

tribe takes ideas from all of them. It doesn't exclude them either: an ACP
agent or an MCP server can join tribe as a participant through an adapter.

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
3. Agent Client Protocol. https://agentclientprotocol.com
4. Model Context Protocol. https://modelcontextprotocol.io
