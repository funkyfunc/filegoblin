---
description: How to design CLI arguments, flags, and outputs.
---

# Master CLI Design Philosophy

When building or reviewing CLI applications (like `filegoblin` or generic Rust tools), adhere to these principles synthesized from clig.dev and bettercli.org.

## 1. Human-First Interaction
- **Conversation as the Norm**: Users learn through exploration. The CLI should act as a helpful guide rather than a strict enforcer.
- **Empathy in Design**: Provide help, suggest corrections for typos (e.g., "Did you mean `--split`?"), and ensure the user feels empowered.
- **Intelligent Defaults**: Make the default behavior the right thing for *most* users. If a feature is difficult to enable, people won't use it.
- **Say (Just) Enough**: Respect the user's attention. Don't hang silently for minutes, but don't drown them in debugging output either. Find the balance.

## 2. Robustness and Predictability
- **Exit Codes**: Always return `0` on success and non-zero on failure. This is non-negotiable for scripts.
- **Standard Streams**:
  - **`stdout`**: Primary output *only*. This is what gets piped into the next command.
  - **`stderr`**: Messaging, logs, warnings, progress bars, and errors. This prevents metadata from corrupting piped pipelines.
- **Consistency**: Adhere to established terminal conventions. Use standard flags (`-a` for all, `-f` for force, `-v` for verbose/version). The terminal is hardwired into our fingers; don't fight it.
- **Dry Runs & Confirmations**: For destructive actions, either prompt for confirmation (when interactive) or require a `-f`/`--force` flag (when scripted). Use `--dry-run` (`-n`) for complex operations.

## 3. Composability: Simple Parts That Work Together
- **Machine vs Human Output**: If outputting human-readable data (colors, tables, animations) breaks machine readability, detect if the output is a TTY.
- Offer `--plain` para tabular data parsing.
- Offer `--json` flags to provide strictly structured, machine-parsable state without resorting to `awk`/`sed` hacks.

## 4. Documentation and Help
- **Accessible Help**: `-h` and `--help` must always display help, regardless of other inputs.
- **Contextual Stacking**:
  - `cmd` (no args): Brief description and one or two examples.
  - `cmd --help`: Full detailed help.
- **Lead with Examples**: Users scan for examples over reading paragraphs. Show common use cases at the very top.
- **Actionable Errors**: Treat errors as documentation. Never just say "Failed." Say "Failed: Cannot write to file. Try running with sudo or changing permissions." Keep the signal-to-noise ratio extremely high.

## 5. Visual Aesthetics (Vibe)
- **Color**: Use color with intention. Highlight key data, use red for errors. But respect the `NO_COLOR` environment variable or non-TTY pipes to automatically disable it.
- **Layout**: Increase information density intelligently. ASCII art, tables, and spacing make scanning easier.

## 6. Flag & Argument Architecture
- **Prefer Flags to Arguments**: `cmd --file data.txt` is much safer and easier to scale than `cmd data.txt` if you ever need to add more functionality.
- **Long vs Short**: Every flag must have a long version (`--all`). Only use short flags (`-a`) for the absolute most common operations to avoid polluting the namespace.
- **Never Require a Prompt**: Prompts are great for humans, but destroy automation. Every prompt must be bypassable via a flag.

## 7. Lifecycle and Configuration
- **The Machine Belongs to the User**: Your CLI is a guest. Leave no trace without permission. Provide clean uninstalls, don't litter the filesystem, and support users who stay on older versions.
- **Configuration Layering**: Support a predictable hierarchy where explicit CLI flags override Environment Variables, which in turn override Configuration files.
- **Robust Booleans**: For environment variables, accept multiple truthy/falsy states (e.g., `1`, `TRUE`, `True`, `ENABLED` vs `0`, `FALSE`, `DISABLED`).

## 8. Networking and Proxies
- **Centralize Networking**: Keep all HTTP/network calls flowing through a single module to ensure consistent configuration.
- **Respect Standard Proxies**: Explicitly respect `HTTP_PROXY`, `HTTPS_PROXY`, and `NO_PROXY`.

## 9. Analytics and Telemetry
- **Fail-Fast and Last**: Analytics requests must never hang the CLI. Fire them asynchronously at the bleeding end of execution with a strict timeout (e.g., 2 seconds).
- **Facade APIs**: Never ping third-party trackers (like Google Analytics) directly from the CLI. This triggers firewalls and looks like malware. Proxy telemetry through your own infrastructure facade API.
- **Anonymize Ruthlessly**: Do not collect file paths or repo names—they often contain highly sensitive credentials or company names.
