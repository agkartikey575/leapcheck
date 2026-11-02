# LeapCheck

**Check before you leap.** Know exactly what will break before you upgrade PHP (and, soon, Node.js and Python), including old libraries hidden inside your plugins and dependencies.

## Repository layout
- `engine/`: Rust core engine (scanning, hidden-library detection, run tracing, static result export)
- `tools/php/`: PHP analyzer dependencies (PHP_CodeSniffer + PHPCompatibility)
- `docs/`: scope, architecture, research (`docs/archive-einvoice/` holds the earlier e-invoicing research)
- `company/`, `.claude/agents/`: the AI company: agents, org chart, roadmaps, task board

## Status
Phase 1 in progress: WordPress/PHP, launching before PHP 8.2 end of support (31 Dec 2026).
