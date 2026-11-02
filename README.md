# LeapCheck

**Check before you leap.** Know exactly what will break before you upgrade PHP (Node.js and Python coming later), including old libraries hidden inside plugins and dependencies.

## Repository layout
- `engine/`: Rust engine: scanning, hidden-library detection, run tracing, static result export
- `engine/rules/`: data-driven detection rules
- `tools/php/`: PHP analyzer dependencies (PHP_CodeSniffer + PHPCompatibility)
- `plugin/`: the free WordPress plugin
- `docs/`: architecture and interfaces

## Status
Early development. First release targets WordPress sites ahead of PHP 8.2 end of support (31 Dec 2026).
