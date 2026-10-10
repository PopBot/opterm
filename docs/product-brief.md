# Opterm product brief

The user confirmed this terminal-focused goal on 2026-10-09. The original five accepted foundations remain in [the design interview](design.md); Q54-Q83 settle the terminal-first scope and UI/settings direction. The design interview is closed; the next useful step is a working prototype.

## The goal

Create a fast, lightweight, reliable everyday terminal for macOS, Linux, and native Windows, with built-in session management that makes running existing AI CLI agents convenient.

The experience should be straightforward: open quickly, start a shell or agent, work responsively, close a view while the work continues, and return to the same live session. People can inspect and deliberately take control of agent sessions. Clear settings and a coherent UI are part of the product, not details left until after the backend.

Speed means reducing terminal overhead in startup, input/output handling, session creation, and reattachment. Measurements verify these outcomes and identify tradeoffs; they do not justify expanding the product into a larger data-management system. Opterm works offline without a required account; agents retain their own provider/network needs.

## What persistence means

| Need | Why it exists | What provides it |
| --- | --- | --- |
| Keep live shells/agents running after a view closes | Built-in tmux-like continuity | An independent background session service holding live state |
| Remember preferences and workspace layout | Restore the user's terminal setup | Global preferences and session profiles in readable TOML with a settings UI; simple layout records |
| Keep past output after reboot | Review earlier work | Saved history, with recording controls and bounded retention |
| Recover recorded direct-command acceptance/results after service failure | Support the previously discussed durable execution API | A durable ledger with explicit recovery semantics; deferred from the first usable version |

SQLite was selected for local metadata and command bookkeeping, while output would use separate files. SQLite is an [embedded database without a separate server](https://sqlite.org/serverless.html); it is a reasonable [local application storage option](https://sqlite.org/whentouse.html). The concern is how much bookkeeping, coordination, retention, and export behavior the first build takes on, rather than the existence of a SQL server to operate.

Q54 accepts a terminal-first usable version with multiple windows and tabs, both split orientations, renamable tabs, keyboard shortcuts, session profiles, custom profile themes, live-session continuity, and simple preference/layout/history files. SQLite, the durable command ledger, public execution interfaces, and indexed history/export are deferred. This changes delivery order rather than promising the earlier durability guarantees through custom files.

## Accepted specification and validation

- **UI structure and workflows:** workspace/session navigation, tabs/panes, agent/session states, observation/takeover, closing versus terminating, reconnection, and clear error/recording states.
- **Visual specification:** layout wireframes, information hierarchy, typography, themes/colors, spacing, focus/selection, and platform conventions, including input and accessibility requirements.
- **Settings model:** appearance, shortcuts/clipboard, shell launch configuration, startup/session behavior, history controls, and agent access; define scopes, defaults, storage/editing, validation, and when changes take effect.

The current [UI/settings specification](ui-settings-spec.md) records the accepted first-version scope, GUI/TOML settings model, and compact reference-based layout: small title/tab chrome above one continuous terminal surface, with splits and additional actions available on demand. Opterm must be standalone; the latest instruction supersedes earlier Crateon styling reuse. Use independently selected fonts and neutral Dark/Light presets, with no related branding, design dependencies, or integrations. Q65-Q68 settle startup restoration, profile management, custom theme fields, and Save/Cancel. Q69-Q83 settle defaults, storage, clipboard/history behavior, shortcut bindings, titles, split profiles, and paste. Routine reversible details belong in the prototype; toolkit selection and native validation require evidence.

## Development workspace

The canonical project is `/home/jarod/Dev/extdev/projects/opterm`, as selected by the user. The repository, design documents, glossary, and RustRover project configuration are kept here. The directory was renamed from `optimized-terminal` on 2026-10-09 at the user's request.
