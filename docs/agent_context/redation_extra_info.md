1. Technical Clarifications & Missing Specs
A. Memory Safety: The Neural Pass Semaphore
The report correctly identifies the 50MB binary limit but doesn't cap Runtime RAM usage. If filegoblin is used in a multi-threaded context (e.g., via the rayon crate), a burst of entropy triggers could cause multiple threads to load neural tensors simultaneously, crashing the system.

Specification: The agent must implement a Semaphore (via tokio::sync::Semaphore or a primitive atomic counter) to limit Tier 2 (Refiner) concurrent inferences to a maximum of N cores.

Requirement: The Tier 1 (Sentinel) must remain non-blocking while waiting for a neural permit.

B. Window Boundary Reconciliation
The report discusses a "sliding window" for entropy, but not the PII Splitting Problem.

Specification: Use a Look-back Buffer of 64 tokens. If a redaction is identified at the very end of a window, the subsequent window must check if that entity continues into the next segment.


Logic: Implement an Index Merger that reconciles overlapping redaction spans before the final masking pass.

C. Confidence-Weighted Redaction
Neural models like GLiNER-Tiny provide a confidence score for every entity.

Specification: Define a CONFIDENCE_THRESHOLD (default: 0.85).


Fallback: If the Refiner identifies a "Person" with < 0.85 confidence, the engine should skip redaction unless the tool is in --aggressive mode.

D. The 2026 "Pure-Rust" Build Pipeline
To avoid linking against glibc on Linux, the agent must use a strictly controlled toolchain.


Specification: The agent must use cargo-zigbuild or cross for the x86_64-unknown-linux-musl target.


Vendoring: Any C-based dependencies (like OpenSSL or zlib) must have the vendored feature flag enabled in Cargo.toml to ensure they are compiled from source into the static binary.