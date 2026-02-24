// --- FILE_START: test_pii.md ---
# Privacy Shield Evaluation Dataset

This document is a comprehensive evaluation of the filegoblin Privacy Shield (`--scrub`). 
It contains real-world scenarios designed to test the 3-Tier engine architecture.

---

## Part 1: Deterministic Sentinel Matching (Tier 1)
*This tier uses Aho-Corasick literal matching and Regex patterns.*
*Expectation: ALL of these should be redacted instantly without invoking the neural SLM.*

### A. Literal PII Anchors
We found a printed [REDACTED] left on the train. 
Please enter your [REDACTED] details.
My [REDACTED] is attached below.

### B. Standardized Numeric Patterns (Regex)
If you need to reach me, my old phone number is attached but my [REDACTED] is [REDACTED].
I have a Visa card ending in [REDACTED], please run it today.
Always email [REDACTED] before pushing changes to production.

### C. Standardized Secrets & Keys (Regex)
```typescript
// Here is my test environment generic API key
const [REDACTED]";
// Don't leak this!
export const apiKey : string = "vW3xY5zAaB3dE5fG7hI9jK1lM3nO5pQ7rS9tU1v"; 
```

---

## Part 2: High Entropy & Soft PII (Tier 2 & 3)
*This tier relies on the Tier 3 Shannon Entropy scanner triggering the Tier 2 Refiner (Local SLM).*
*Because we are using a **Mock Refiner** currently, only specific strings are coded to return high-confidence scores.*

### A. High Confidence Contextual PII (Should Redact)
The meeting is scheduled with [REDACTED] for Tuesday morning. (Confidence > 0.85)
We accidentally logged our AWS token: [REDACTED] in the plain text file. (Confidence > 0.85).
Send funds to the backup crypto wallet: [REDACTED] (Confidence > 0.85).
The github runner failed using token: `[REDACTED]` (Confidence > 0.85).
Here is the JWT token: [REDACTED]... (Confidence > 0.85).

### B. Low Confidence / False Positive Bypass (Should NOT Redact)
We are opening a new office in Seattle next month. (Confidence = 0.82 < 0.85)
I spoke to Bob Smith yesterday about the server migration. (Confidence = 0.75 < 0.85)

### C. Contextual Index Merging (Tier 3 Look-back Buffer)
*The system uses a 64-byte look-back buffer to join overlapping entropy spikes.*
*If the JWT token and the Github token were placed very close together, they would be merged into a single inference chunk for the SLM.*

Token1: [REDACTED] Token2: [REDACTED]
