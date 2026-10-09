---
status: accepted
---

# Separate local control messages from streamed terminal output

Opterm's desktop and agent interfaces need versioned control operations and incremental access to raw command and terminal output. Use length-prefixed typed frames with JSON control and binary output over same-user Unix sockets on macOS/Linux and named pipes on Windows, negotiating versions/capabilities while CLI and MCP adapters translate results into their JSON formats. We accept framing and adapter complexity in exchange for preserving raw output and avoiding mandatory JSON encoding on the service's output-streaming path; detailed schemas and bounds require prototype validation.
