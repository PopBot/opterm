# Opterm

A lightweight cross-platform terminal for macOS, Linux, and Windows, with built-in session management for everyday shells and existing AI CLI agents.

## Current status

The product design is documented and the interview is closed. This repository contains specifications and illustrative mockups; application code and benchmarks have not been implemented yet.

## Project documents

- [Product brief](docs/product-brief.md)
- [UI, settings, and first-version specification](docs/ui-settings-spec.md)
- [Terminal-first prototype plan](docs/prototype-plan.md)
- [Decision history](docs/design.md)
- [Approved layouts and historical mockups](docs/design/mockups/README.md)
- [Glossary](GLOSSARY.md)

## Implementation direction

Use Rust for the shared core and independent background session service. Follow the accepted thin terminal layout and implement standard windows, tabs, splits, profiles, themes, shortcuts, settings, and detach/reattach workflows. Evaluate the UI toolkit through a working terminal slice and native platform checks.

Start with simple configuration, layout, and bounded history files. SQLite, a durable direct-command ledger, public execution API/JSON CLI/MCP integration, and indexed history/export are deferred from the first usable version.

Live sessions can survive closing or crashing desktop views while the background service remains alive. This does not promise arbitrary process survival through reboot or service failure, and commands must not be automatically replayed.

Opterm uses independent styling. Historical other-product design references are retained only as design records and must not become application dependencies or shipping assets.
