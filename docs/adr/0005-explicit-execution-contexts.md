---
status: accepted
---

# Use explicit task contexts and create shells when needed

Direct commands need reproducible working-directory and environment settings without requiring an interactive shell for every task. Each task will own a persistent execution context, with interactive shells created only when needed and shell-internal changes synchronized through explicit context updates. We accept that shell changes are not automatically shared with direct commands in exchange for clearer state ownership and lower unnecessary startup overhead.
