---
description: How to architect, maintain, and scale the high-performance recursive web crawler (Horde Mode).
---

# Web Crawler Architecture & Engineering Rules

This document synthesizes the core engineering principles required when modifying or extending the `filegoblin` recursive web crawler (`--horde` mode). Follow these strict architectural guidelines to ensure memory safety, high performance, and ethical compliance.

---

## 1. Concurrency & Orchestration
*   **Async First:** The crawler must be heavily I/O bound. Rely exclusively on `tokio` for non-blocking execution.
*   **Worker Pool Pattern:** Drive concurrency using a central channel manager (`tokio::sync::mpsc`) that distributes URLs from the frontier queue to worker tasks.
*   **TCP Throttling:** Never unbound concurrency. Always use a `tokio::sync::Semaphore` to cap the total active concurrent HTTP requests, avoiding local OS descriptor limits and target server DDoS alarms.

## 2. Graph Traversal State
*   **Breadth-First Search (BFS):** Always traverse the web graph horizontally (BFS). Depth-First Search (DFS) is strictly prohibited as it easily traps the crawler in infinite dynamic directory loops (e.g., infinite calendar link generation).
*   **Lock-Free Deduplication:** Use `dashmap::DashSet` to store the visited URL history. Standard `Mutex<HashSet>` will cause severe lock contention bottlenecking performance.
*   **Massive Scale Mitigation:** If the memory footprint of `DashSet<String>` becomes a bottleneck at >1-million URLs, migrate the history tracker to a probabilistic structure like a **Bloom Filter**.

## 3. Link Filtering & Normalization
Before adding any discovered URL to the frontier, it must be validated:
*   **Domain Scoping:** Compare the parsed URL's `host()` to the original seed domain. Outbound links must be discarded to keep the crawler contained.
*   **Canonicalization:** Enforce strict normalization using `urlnorm`: lowercase the host, strip trailing slashes, remove `#fragments`, and strip tracking parameters (`utm_`).
*   **Relative Paths:** Ensure relative paths (`/about`) are properly joined against the current page's base URL using the `url` crate.

## 4. Ethical Politeness (MANDATORY)
*   **Robots.txt:** Webmaster rules dictate crawl boundaries. Use the `robotxt` crate to verify every path before fetching. Cache rules per-domain for 24 hours.
*   **Domain-Keyed Rate Limiting:** Prevent server overload using the Generic Cell Rate Algorithm (GCRA) via the `governor` crate. Limits must be applied *per domain*, not globally.
*   **Exponential Backoff:** Transient errors (500, 503, 504) must be retried using exponential backoff with random "jitter" (`tokio-retry`) to prevent thundering herd scenarios. Honor `429 Too Many Requests` by backing off immediately.

## 5. Extraction & Translation Pipeline
*   **Noise Reduction:** Do not convert raw HTML strings blindly to Markdown. Use readability heuristics (e.g., `readability-rs` or `scraper` extraction of `<article>`/`<main>`) to strip navigation menus, footers, and sidebars.
*   **Conversion Standard:** Rely on `html-to-markdown-rs` to produce LLM-optimized, high-fidelity CommonMark.
