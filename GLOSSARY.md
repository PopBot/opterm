# Opterm

Opterm is a cross-platform desktop terminal for people and existing AI agents. It provides live terminal sessions that remain available independently of the desktop interface.

## Language

**Desktop terminal**:
The interface through which a person views and interacts with Opterm terminal sessions. Closing or losing this interface does not, by itself, end those sessions.
_Avoid_: Shell, session

**Session service**:
The part of Opterm responsible for managing live terminal sessions independently of the desktop terminal. It is available to both the desktop terminal and agents.
_Avoid_: Desktop terminal, shell

**Terminal session**:
A live terminal environment whose running program remains available when the desktop interface closes or fails. This continuity does not promise that running programs survive a host reboot or failure of the session service.
_Avoid_: Window, tab, saved history

**Session attachment**:
A connection through which a person or agent accesses an existing terminal session. Ending an attachment is distinct from ending the session.
_Avoid_: New session, session termination

**Session profile**:
A named preset used to start a shell or existing CLI agent with chosen launch settings and an optional profile theme. It does not represent a running terminal session or restore an ended process.
_Avoid_: Live session, process backup, workspace

**Profile theme**:
A named appearance preset that a session profile can select, including custom values within Opterm's supported theme options. It changes terminal appearance rather than the hosted program or agent's behavior.
_Avoid_: Agent persona, executable plugin, session profile

**Session history**:
Saved terminal output, direct-command invocations and results, and session/workspace metadata that remain available after a host reboot, subject to recording and retention controls. Restoring history does not resume the program that produced it or guarantee a complete transcript.
_Avoid_: Live session, process restoration

**Agent workflow overhead**:
The time and resources Opterm adds to starting or attaching to sessions, dispatching commands, and collecting their results. This is distinct from the computation performed by a command and the time an AI model takes to respond.
_Avoid_: Command execution speed, model inference speed

**Existing CLI agent**:
An independently developed AI agent that runs through a command-line interface and can be hosted in Opterm. Hosting an agent does not imply that it uses Opterm's execution API.
_Avoid_: Built-in Opterm agent

**Direct command run**:
An Opterm-managed command invocation that does not require an interactive terminal session and returns its output and exit result. It is distinct from sending input to a shell already running in a terminal session.
_Avoid_: Terminal session, shell input

**Agent task**:
A unit of work assigned to an existing CLI agent, with its own execution context and terminal sessions created when needed. Task separation does not, by itself, imply filesystem or security isolation.
_Avoid_: Terminal session, workspace

**Input controller**:
The sole participant permitted to send input to, and resize, a particular terminal session at a given time. Input control can be deliberately transferred between an agent and a person.
_Avoid_: Observer, session owner

**Session observer**:
A participant who inspects a terminal session without controlling its input or dimensions. Observation alone does not interrupt the running program or transfer input control.
_Avoid_: Input controller

**Human takeover**:
An explicit transfer of terminal session input control to a person, preventing simultaneous human and agent input while leaving running programs untouched. Pausing a hosted CLI agent's internal actions requires a separate integration.
_Avoid_: Observation, automatic agent pause

**Execution context**:
The working-directory and explicit environment settings owned by an agent task and used by its direct command runs; values marked secret must be supplied again after restart. Changes inside an associated interactive shell enter this context only through an explicit update.
_Avoid_: Shell state, terminal session, security sandbox

**Workspace**:
A named local group of related terminal sessions and direct command runs, with a default working directory and saved desktop layout. It does not require a Git repository or imply filesystem isolation.
_Avoid_: Git repository, security sandbox, individual terminal session

**Unknown execution result**:
A command result that Opterm cannot reliably determine after a connection or service failure. It requires deliberate recovery or restart rather than automatic command replay.
_Avoid_: Success, failure, safe to retry

**Output cursor**:
A position in a particular session or command output stream from which a client can request subsequent bytes. Resuming at that position depends on the requested output still being available.
_Avoid_: Terminal cursor, complete transcript

**History gap**:
A range of output that is unavailable because it was not retained or could not be recorded. Opterm reports the gap so a person or agent can distinguish partial history from complete history.
_Avoid_: Empty output, successful complete replay

**Transcript durability**:
The extent of a command or session's output saved to survive the promised failure conditions. A saved command result does not, by itself, mean all of its output is durable.
_Avoid_: Command completion, live process survival

**Agent grant**:
Permission for an API client to perform specified operations on an assigned task and its sessions or command runs. It governs access through Opterm's interfaces and does not restrict a hosted program's OS-user privileges.
_Avoid_: OS sandbox, unrestricted workspace access

**Secret environment override**:
An explicitly marked environment value supplied for running work but excluded from restored environment configuration. This designation does not guarantee that the value cannot appear in command output or other recorded data.
_Avoid_: Automatically detected secret, complete history redaction

**Live terminal state**:
A terminal session's current screen content, cursor, modes, and dimensions, maintained independently of the desktop view. It stays current while the desktop is detached and is distinct from saved output history.
_Avoid_: Transcript, screenshot, saved session history

**Terminal view**:
A presentation of a terminal session's current screen within the desktop interface. Closing a tab or pane containing a view detaches it rather than ending the running session.
_Avoid_: Terminal session, session service

**Submission ID**:
A task-scoped identifier for a request to start a direct command run, through which repeated submissions refer to the original request. An expired or unrecoverable identifier does not authorize a replacement run.
_Avoid_: New command invocation, session ID

**History export**:
A consistent snapshot of selected saved history, its retained output, and any known gaps. It represents historical data rather than live work or an instruction to restart commands.
_Avoid_: Live session backup, process restoration

**Screen snapshot**:
A read-only structured representation of a terminal session's current text, cell positions, cursor, dimensions, and relevant modes. It represents the visible terminal state rather than inferred command meaning or complete saved history.
_Avoid_: Screenshot, complete transcript, command result

**Run context**:
The working directory and effective environment captured for a direct command run when Opterm accepts it. Later task-context changes affect later submissions, while secret values remain excluded from persisted environment configuration.
_Avoid_: Mutable task context, shell state, filesystem isolation

**Execution outcome**:
Opterm's report of whether a direct command launched and what happened to its managed program, including observed exit, verified termination, or an unknown result. Cancellation requests, output completeness, and transcript durability are reported separately.
_Avoid_: Complete transcript, cancellation request, inferred success
