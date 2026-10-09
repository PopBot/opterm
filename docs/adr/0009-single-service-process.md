---
status: accepted
---

# Start with one session-service process

Opterm's initial session service will own live sessions and command runs in one process, independently of the desktop interface. We accept that service failure can end live work in exchange for lower initial resource and lifecycle complexity than separate per-session owner processes. Desktop close/crash survival remains required; stronger service-crash isolation is deferred until measurements and failure tests justify its cost.
