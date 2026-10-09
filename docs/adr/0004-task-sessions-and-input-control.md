---
status: accepted
---

# Give agent tasks separate sessions and explicit input control

Sharing a terminal between concurrent tasks or controllers can mix input and shell state. Each agent task receives its own session and working environment by default, while people can observe and deliberately take control; each interactive session permits one active controller of its input and dimensions. We accept the resource cost of separate sessions and explicit handoff coordination to reduce interference, without claiming filesystem isolation or the ability to pause arbitrary agents' internal tools.
