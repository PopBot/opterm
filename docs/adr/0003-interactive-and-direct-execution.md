---
status: accepted
---

# Provide interactive sessions and direct command execution

Existing CLI agents and interactive applications need terminal behavior, while ordinary agent commands benefit from explicit stdout, stderr, and exit results. Opterm will support both interactive terminal sessions and direct command runs through the session service, preferring direct execution when terminal interaction is unnecessary. We accept maintaining two execution paths in exchange for clearer command results and an opportunity to reduce agent workflow overhead; [ADR 0005](0005-explicit-execution-contexts.md) defines their state relationship.
