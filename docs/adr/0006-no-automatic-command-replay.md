---
status: accepted
---

# Do not automatically replay commands with uncertain results

A connection or service failure can leave Opterm unable to determine whether a command already performed its effects. Opterm will reattach to existing work when possible, otherwise report lost or unknown execution state and require deliberate reruns; a repeated submission ID retrieves its original run, rejects changed parameters, or reports expired/unknown state instead of launching replacement work. We accept interrupted workflows requiring recovery in exchange for avoiding duplicate execution caused by automatic retries, without promising exactly-once command effects through arbitrary failures.
