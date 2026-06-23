# The North Star: The Soul of filegoblin

## What This Is

filegoblin is the ultimate **Swiss Army knife** for context ingestion. 

The real world is messy. Data is trapped in bloated PDFs with broken tables, nested deep inside DOCX files, buried in legacy Excel spreadsheets, spread across massive unstructured source code repositories, and dynamically rendered on web pages. Language Models, however, demand clean, structured, high-signal text to reason effectively.

filegoblin exists to bridge that gap. It takes the chaotic, heterogeneous, messy world of human data and effortlessly translates it into something that an LLM can actually understand.

When a developer thinks, *"I need this entire project, plus these two PDFs and that API documentation website in my prompt,"* they shouldn't have to string together five different python scripts, deal with OCR dependencies, or manually copy-paste. They just unleash the goblin.

---

## The Identity

`filegoblin` embraces the **Mischievous Librarian** archetype. We don't just "process" data; we **feast** on it.

The tool should feel voracious but incredibly precise. It tears through thousands of files instantly but delicately extracts the exact structural meaning (like AST signatures or table cells) without losing fidelity.

This identity anchors our design:
- **Zero Friction:** A single, statically linked Rust binary. No `pip install`, no Node.js limits, no Docker daemons required just to parse a file.
- **High-Signal:** We don't just concatenate text. We use model-specific flavors (XML tags, ASCII maps) to prevent LLM attention drift. 
- **Privacy as a Right:** The goblin eats data, but it doesn't leak secrets. Local, zero-latency redaction and PII scrubbing ensure that sensitive data never makes it to the cloud provider's API.

---

## The Core Philosophy

### 1. It Just Works (The Duality of Use)
Like a great command-line tool, filegoblin scales with the user's intent.
- **Casual Use:** `filegoblin document.pdf > prompt.txt`. No manual needed.
- **Power User:** `filegoblin ./src --horde --exclude "*.test.rs" --compress aggressive --scrub --tokens --flavor anthropic`. Every knob is available when you need absolute control over your context window.
- **Pipeline Native:** Output flows cleanly to stdout. Progress bars disappear when piped. Exit codes are predictable.

### 2. Format Invisibility
The ultimate goal is that the user stops thinking about file extensions. Whether they pass a `.docx`, `.pdf`, `.rs`, `.xlsx`, or an `https://` URL, filegoblin figures out the parser, extracts the semantic meaning, normalizes it (like converting wide tables into "Sequence of Records"), and outputs text. 

### 3. Absolute Trust and Privacy
Enterprise users handle sensitive codebases and legal documents. They must trust that when they run filegoblin:
1. It won't crash on a massive 100,000 file monorepo.
2. It won't send their PDF to a third-party cloud API for parsing.
3. It will catch API keys and PII before it hits stdout.

### 4. Speed is a Feature
Context generation is the bottleneck of AI development loops. If the developer has to wait for Python environments to resolve or for heavy ML vision models to boot up just to parse a PDF, we failed. Filegoblin must be instantaneous. We rely on compiled Rust, concurrent Rayon pipelines, and zero-cost abstractions to make context generation feel magical.

---

## What We Are Not

- **Not a wrapper around external APIs.** We parse locally. We don't rely on Adobe PDF Extract or external OCR cloud services. 
- **Not a heavy runtime.** We do not require Python or Node.js. If a parser requires massive system dependencies that break CI environments, it doesn't belong in the goblin's stomach (unless it's compiled to a pure-Rust WASM plugin).
- **Not just a concatenator.** `cat * > out.txt` already exists. Filegoblin adds value through minification, structural anchoring, semantic pruning, and privacy.

---

*This document defines **why** we build filegoblin. For technical implementations, refer to `ARCHITECTURE.md` and `PRD.md`.*
