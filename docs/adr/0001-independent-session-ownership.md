---
status: accepted
---

# Own live sessions independently of the desktop interface

Opterm needs live sessions to survive desktop window closure and desktop application crashes, and agents need access without an open window. A separate session service will own live sessions, with the desktop terminal attaching as a client. This accepts service lifecycle and communication overhead in exchange for keeping running programs independent of the desktop interface.
