---
status: accepted
---

# Separate execution records from bulk output

Opterm must recover execution records without forcing high-volume terminal output through the same storage path. Store metadata and execution bookkeeping in SQLite and output in segmented append-only files, persisting command acceptance before effects and completion bookkeeping before acknowledging completion while batching ordinary output writes. We accept cross-store recovery coordination and a potentially lost unflushed output tail in exchange for separating bookkeeping durability from bulk-output throughput; a saved result does not imply a fully durable transcript.
