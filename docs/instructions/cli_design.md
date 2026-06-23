# CLI Design Philosophy

This document serves as the baseline specification for generating and maintaining Command Line Interface (CLI) applications. The downstream coding agent MUST internalize and strictly adhere to these rules. The objective is to produce tools that are highly composable, possess exceptional local-first performance, and exhibit deep user empathy.

## 1. Core Philosophy
- **Composability**: Expect the output of every program to become the input to another. Data MUST be easily processable by standard text utilities (`grep`, `awk`, `jq`).
- **Local-First & Fast**: Execution speed is mandatory. Defer heavy network calls or complex initializations until strictly necessary. Provide sensible defaults so commands work without exhaustive configuration.
- **User Empathy**: Favor descriptive, standard naming conventions over esoteric abbreviations. Provide actionable error messages rather than raw stack traces.
- **Backward Compatibility**: Command signatures (flags, argument order, output format) are a binding contract. If a feature is retired, use a prolonged deprecation cycle with non-fatal warnings before removal.

## 2. Arguments, Flags, and Options
- **POSIX Compliance**: Use standard parsing libraries (`clap`, `cobra`) instead of ad-hoc splitting. Support short options (`-a`), long options (`--all`), option clustering (`-abc`), and standard value attachments.
- **Positional vs. Flags**: Use positional arguments for mandatory operational targets (e.g., `cp <src> <dest>`). Use flags for optional modifiers. Do not require long options for basic use cases.
- **Subcommand Architecture**: Implement a hierarchical command tree for tools with multiple discrete functions.
  - *Example*: `app user add` should act as its own logical unit with specific positional arguments and local flags.
  - *Local vs Persistent*: Differentiate between local flags (scoped to a subcommand) and persistent flags (global options like `--verbose` or `--config` applied to the root command).
- **Double-Dash Terminator (`--`)**: Must be supported to safely pass positional arguments that begin with hyphens (e.g., `rm -- -r`).
- **Single Hyphen (`-`)**: Must be supported as an operand to explicitly read from `stdin` or write to `stdout` in lieu of a file path.

## 3. Standard I/O, Streams, and TTY
- **Strict Stream Separation**:
  - **`stdout`**: Reserved exclusively for machine-readable data payloads. Never print conversational text or prefixes here.
  - **`stderr`**: Used for all out-of-band communication: diagnostic messages, warnings, logs, interactive prompts, loading spinners, and progress bars.
- **TTY Detection**: The application MUST actively detect if streams are connected to an interactive terminal (`isatty`).
  - **Non-TTY Fallback**: Automatically strip ANSI color codes, disable spinners, and bypass prompts if not in a TTY environment.
  - **Interactive Safeguards**: Never block execution waiting for interactive input in headless environments. Abort with a descriptive error unless a `--force` flag is provided.
- **Terminal Colors Evaluation Hierarchy**: Process rules from highest priority down to the default:

| Priority | Source | Condition | Behavior |
| --- | --- | --- | --- |
| **1 (Highest)** | Flag | `--color=always` / `--color=never` | Immediate adherence. Overrides environment and hardware logic. |
| **2** | Env Var | `CLICOLOR_FORCE != 0` | Force emission of ANSI color codes, bypassing TTY checks. |
| **3** | Env Var | `NO_COLOR` is present and non-empty | Completely suppress all ANSI color output. |
| **4** | Env Var | `CLICOLOR == 0` | Completely suppress all ANSI color output. |
| **5** | Hardware | `isatty` returns false | Suppress color. Output is piped to a file or machine parser. |
| **6 (Lowest)** | Default | TTY is true | Safely emit standard colored output. |

## 4. Exit Codes
- Return `0` strictly for absolute operational success. Never catch a runtime error and exit with `0`.
- Return non-zero (`1-255`) for any failure, warning, or abnormal termination.
- **Centralized Error Handling**: Centralize error handling at the very top of the command execution tree. Catch exceptions/errors globally, log the root cause and diagnostic info directly to `stderr`, map the failure to the most appropriate `sysexits.h` code, and exit.
- **sysexits.h Standards**: Use precise exit codes over a generic `1` where possible:

| Exit Code | POSIX Macro | Architectural Meaning |
| --- | --- | --- |
| 64 | `EX_USAGE` | Command line usage error (invalid flags, missing arguments). |
| 65 | `EX_DATAERR` | Data format error (malformed input, unreadable JSON). |
| 66 | `EX_NOINPUT` | Cannot open input (file does not exist or lacks permission). |
| 68 | `EX_NOHOST` | Host name unknown (DNS resolution failure). |
| 69 | `EX_UNAVAILABLE` | Service unavailable (routing failure, 503 response). |
| 70 | `EX_SOFTWARE` | Internal software error (unhandled panics, null pointers). |
| 71 | `EX_OSERR` | System error (out of memory, fork failed). |
| 73 | `EX_CANTCREAT` | Cannot create output file (locked states, missing directories). |
| 77 | `EX_NOPERM` | Permission denied (lack of filesystem ACLs). |
| 78 | `EX_CONFIG` | Configuration error (unparseable or conflicting config file). |

## 5. Environment and Configuration
- **Configuration Hierarchy**: Resolve configuration variables using strict precedence:
  1. Command-Line Flags (Highest Priority)
  2. Environment Variables
  3. Local Configuration Files (`./.apprc.json`)
  4. Global Configuration Files
  5. Application Defaults (Lowest Priority)
- **XDG Base Directory Specification**: Never hardcode configuration files to `$HOME/.myapp/`. Use standard XDG variables with proper fallbacks:
  - `XDG_CONFIG_HOME`: Configuration files. Fallback: `$HOME/.config/`
  - `XDG_DATA_HOME`: General portable data files. Fallback: `$HOME/.local/share/`
  - `XDG_STATE_HOME`: Operational state (logs, history). Fallback: `$HOME/.local/state/`
  - `XDG_CACHE_HOME`: Non-essential regenerable data. Fallback: `$HOME/.cache/`
- **Cross-Platform Mappings**: When compiling cross-platform binaries, map XDG concepts to standard OS equivalents (e.g., Windows: `%APPDATA%` for config, `%LOCALAPPDATA%` for cache/data/state; macOS: `~/Library/Application Support/` for config/data, `~/Library/Caches/` for cache).

## 6. Help and Discoverability
- **Standard Help Output**: Dynamically generate help text (`-h` / `--help`) from the parser schema. Include Usage, Description, Arguments, Flags (Local vs. Persistent), Subcommands, and Examples.
- **Autocompletion**: Natively support generating shell autocompletion scripts (bash, zsh, fish) and support context-aware completion where applicable.
- **Actionable Errors**: Suggest corrections for typos (e.g., "unknown command 'updaet'. Did you mean 'update'?").

## 7. Networking and Proxies
- **Centralize Networking**: Keep all HTTP/network calls flowing through a single module to ensure consistent configuration.
- **Respect Standard Proxies**: Explicitly respect `HTTP_PROXY`, `HTTPS_PROXY`, and `NO_PROXY`.

## 8. Analytics and Telemetry
- **Fail-Fast and Last**: Analytics requests must never hang the CLI. Fire them asynchronously at the bleeding end of execution with a strict timeout (e.g., 2 seconds).
- **Facade APIs**: Never ping third-party trackers (like Google Analytics) directly from the CLI. Proxy telemetry through your own infrastructure facade API.
- **Anonymize Ruthlessly**: Do not collect file paths or repo names—they often contain highly sensitive credentials or company names.

## 9. Anti-Patterns to Avoid
- **The "God File" Monolith**: Do not put everything in `main.go` or `main.rs`. Decouple command parsing from business logic.
- **Log Pollution in `stdout`**: Never print INFO, WARN, DEBUG, or ERROR logs to `stdout`.
- **Interactivity Deadlocks**: Never wait for `stdin` confirmation on destructive actions without a programmatically passable bypass flag (e.g., `--yes` or `--force`).
- **Breaking Backward Compatibility**: Never arbitrarily change flag names or argument orders in minor version bumps.
- **Reinventing the Wheel**: Do not write custom Regex parsers for flags or custom formats for config files. Use established libraries (`clap`, `cobra`, JSON, TOML, YAML).
