# Privacy Shield Redaction Test File

This file contains various forms of text to test the `--scrub` functionality of the filegoblin Privacy Shield.

## 1. What SHOULD be caught (Currently Implemented)
The current Tier 1 (Sentinel) pass uses Aho-Corasick for extremely fast, deterministic matching of known high-risk keyword patterns (like "SSN" string), and an optimized Regex engine to catch the actual sensitive data digits associated with them.
The following items WILL be redacted:

- My SSN is listed here.
- Please provide your Passport before boarding.
- You can pay with your Credit Card.
- This is my raw number: 123-45-6789 (Regex triggered)
- My old card was 4111-1111-1111-1111 (Regex triggered)
- Contact me at dino@filegoblin.io (Regex triggered)

## 2. What SHOULD NOT be caught (By Design for Tier 1)
The Sentinel pass is designed to be purely deterministic and extremely fast. It does not understand context.

- "She lived in Seattle." (No anchor keyword, no regex pattern)

## 3. What will be caught in the future (Tier 2 & Tier 3)
In the full production engine (once the `candle-core` SLM tensors are fully statically embedded), the Tier 3 (Heuristic Trigger) will detect the high Shannon entropy of non-matching strings, and pass them to the Tier 2 neural network. The neural network will then identify them contextually as `[REDACTED]`. Currently, the neural step is mocked.

- Jane Doe lives in Seattle. (Soft PII, Name and Location, will be caught by SLM)
- `eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...` (High entropy token, will trigger SLM)

