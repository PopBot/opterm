---
status: accepted
---

# Use Rust for the session service and shared core

Opterm needs a shared systems core for concurrent local sessions on macOS, Linux, and native Windows. We choose Rust for the session service, process management, and shared core, favoring its memory-safety model and reusable terminal ecosystem while accepting learning, build, and platform-integration costs. This decision leaves the desktop UI toolkit open for evaluation.
