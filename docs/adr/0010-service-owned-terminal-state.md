---
status: accepted
---

# Maintain live terminal state independently of views

Detached sessions must continue updating their terminal screen even when saved output is incomplete or expires. The session service will own the live screen, cursor, modes, and dimensions, giving reconnecting views a current snapshot followed by changes while raw-output access remains available. We accept headless parsing, memory, and snapshot-protocol costs in exchange for correct live reattachment without requiring a complete retained transcript; rendering remains the desktop's responsibility.
