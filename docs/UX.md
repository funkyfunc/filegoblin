# filegoblin: The Vibe Spec (UX & Personality) v1.1

**Project Alias:** `fg`
**Vibe Archetype:** The Mischievous Librarian (Helpful, punchy, slightly chaotic but hyper-organized).

---

## 1. Visual Identity (Terminal Aesthetics)
The "Goblincore" aesthetic is earthy, acid-bright, and terminal-native.

### 1.1 Color Palette (ANSI 256 / TrueColor)
* **Primary (Success/Active):** Acid Green (`#A7FF00`) — Progress bars, "Done" messages, selected files.
* **Secondary (Structure):** Earthy Brown (`#8B4513`) or Rust (`#B7410E`) — TUI borders and tree lines.
* **Accent (Warning/PII):** Warning Amber (`#FFBF00`) — Redaction alerts and High Token warnings.
* **Muted (Metadata):** Stone Gray (`#708090`) — File paths and timestamps.

### 1.2 The "ASCII Mascot"
Display this on `--help` or when the TUI (`-i`) launches:

```text
    (o_o)  <-- "I'm hungry for files."
     (W)
   --m-m--  filegoblin v1.5
```

---

## 2. Voice & Tone (The "Goblin" Language)
Avoid dry, corporate language. Use "Goblinisms" for all status updates.

| Standard CLI Action | filegoblin "Goblinism" |
| :--- | :--- |
| **Processing...** | `Crunching...` or `Chewing...` |
| **File converted.** | `Gobbled [file.pdf]! Spat out [file.md].` |
| **Searching directory.** | `Sniffing for files...` |
| **PII Redacted.** | `Scrubbed the secrets.` |
| **Error: Not Found.** | `Pah! The trail went cold. (Not found).` |
| **Copying.** | `Stashing the loot in your clipboard.` |

---

## 3. TUI Layout & Interactivity (Ratatui Design)
The Interactive mode (`fg -i`) is a high-fidelity environment.

### 3.1 The "Hoard" Selector
* **Navigation:** Smooth scrolling with `j/k` (Vim-style).
* **Selection:** Toggled files (`Space`) should "glow" in bold Acid Green with a `*` or `+` indicator.
* **Preview Pane:** Live Markdown view of the highlighted file (or signatures in `--skeleton` mode).

### 3.2 The Progress Bar (The "Teeth")
* **Style:** `Crunching: [vvvvvvvvv-----------] 45%`
* **Animation:** The `v` characters should "jitter" (switch between uppercase/lowercase `V/v`) at 10Hz to simulate chewing.

---

## 4. Error Personalities & Empty States
* **The Empty Hoard:** If `fg` is run on an empty folder, it should say: *"Nothing here but dust and spiders. Feed me a file!"*
* **The "Tough" File:** If a PDF is encrypted, it says: *"This one is too gristly! I need a password to chew on it."*
* **The "Full" Belch (Summary):** Every successful Horde ingestion ends with a summary table showing tokens, files, and redactions.

---

## 5. Implementation Guidance
* **Animations:** Keep them snappy (0.1s).
* **Machine Rule:** If output is piped (`|`), strip ALL colors and ASCII art. Goblins only perform for humans.