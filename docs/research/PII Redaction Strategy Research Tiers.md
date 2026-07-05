# **Architectural Frameworks for High-Performance Local PII Redaction in Rust-Based Hermetic Systems**

The development of the "Privacy Shield" engine for the filegoblin CLI tool represents a significant engineering challenge at the intersection of high-performance systems programming and data privacy compliance. In an era where data breaches can cost institutions upwards of $100 million in remediation and penalties, the imperative to redact Personal Identifiable Information (PII) before it leaves a local environment has never been more critical. For a high-performance tool like filegoblin, which targets file-to-markdown conversion, the redaction engine must achieve a recall rate exceeding 95% while maintaining sub-50ms latency per document. Achieving these goals within a hermetic, zero-dependency, statically linked binary requires a deep evaluation of both algorithmic and neural strategies, particularly as the project moves into the 2025/2026 development cycle.  
The fundamental difficulty in PII redaction lies in the distinction between structured data—such as Social Security Numbers (SSNs), credit card numbers, and email addresses—and "soft" PII, which includes names, organizations, and geographic locations. While structured PII often adheres to deterministic patterns that can be identified via regular expressions or deterministic finite automata, soft PII is characterized by linguistic ambiguity, polysemy, and context dependence. Traditional solutions often rely on heavy Python-based stacks like spaCy or Microsoft Presidio, but these fail the project's hard constraints regarding binary size, static linking, and air-gapped performance. This analysis proposes three distinct architectural tiers designed to meet the rigorous demands of filegoblin's "Privacy Shield."

## **The Baseline Critique: Technical Debt of the ONNX Runtime**

The current baseline for the redaction engine utilizes a hybrid approach: regular expressions for structured data combined with a quantized Distil-PII model (135M parameters) running on the ONNX Runtime (ORT). While this provides a functional starting point, it introduces substantial technical debt that threatens the project's core requirement of a lightweight, hermetic binary. The ONNX Runtime, while highly performant, is a complex C++ project that often requires dynamic shared libraries, complicating the goal of a single, zero-dependency executable.  
Building a statically linked binary for the x86\_64-unknown-linux-musl target using ORT necessitates a delicate dance with the C++ toolchain and memory allocators like mimalloc. The Linux standard library (glibc) is often taken for granted, but its dynamic nature makes it unsuitable for distributing truly freestanding binaries. Using musl-libc as a replacement eliminates these dynamic dependencies but introduces its own set of constraints, such as a significantly smaller default stack size (80 KB) compared to glibc (8 MB). While Rust mitigates this by overriding the stack size to 2 MB, the integration of heavy C++ runtimes like ORT into a musl environment remains a frequent source of "shared library hell" and compilation failures, particularly when native dependencies like OpenSSL are involved.  
Furthermore, the binary size of a 135M parameter model like Distil-PII, even when quantized, typically exceeds the 50MB "Binary UX" target. A DistilBERT-based PII model often results in weights around 70MB to 110MB alone, without considering the overhead of the runtime itself. This suggests that while Distil-PII provides high accuracy (≈96% F1 score), it is an "overweight" solution for a high-performance CLI tool intended for rapid deployment and edge execution.

| Metric | Baseline (Distil-PII / ORT) | Target Requirement |
| :---- | :---- | :---- |
| **Recall (Soft PII)** | 96% | 95%+ |
| **Latency** | 100ms \- 300ms (unoptimized) | \<50ms |
| **Binary Size** | \>150MB | \<50MB (Ideal) / \<100MB (Max) |
| **Static Linking** | Complex (C++ Dependencies) | Zero-Dependency (Pure Rust) |
| **Platform Portability** | Limited by Runtime Bindings | High (musl, msvc, darwin) |

## **Tier 1: The Pure-Rust Zero-Neural Approach**

The Tier 1 approach seeks to achieve high-recall PII redaction by leveraging high-performance algorithms that do not require a neural network runtime. This strategy focuses on algorithmic efficiency, memory locality, and deterministic execution, which are the hallmarks of the Rust ecosystem. The core components of this tier are the Aho-Corasick algorithm for pattern matching and Conditional Random Fields (CRF) for sequence labeling.

### **Aho-Corasick and Linear Time Discovery**

The Aho-Corasick algorithm is uniquely suited for finding multiple PII patterns simultaneously in a single linear scan of the document text. Unlike standard regular expressions, which can suffer from exponential backtracking in certain edge cases, Aho-Corasick builds a finite state machine (FSM) that processes the input text in O(n) time, where n is the length of the text. In the context of 2025 and 2026, the Rust aho-corasick crate has reached a state of extreme maturity, offering SIMD-accelerated prefilters that significantly outperform traditional regex engines for structured PII detection.  
For soft PII, the Tier 1 strategy employs Aho-Corasick to match against large, compressed dictionaries of known sensitive terms, such as names of common organizations or geographic locations. The algorithm's memory usage is configurable; by using a sparse transition representation for the majority of states and a dense representation for states near the root, the FSM can store thousands of patterns within a memory footprint of approximately 60MB. This allows filegoblin to embed extensive look-up tables directly into the binary without the constant memory overhead of a transformer model.

### **Conditional Random Fields for Contextual Soft PII**

While Aho-Corasick excels at exact matches, it struggles with the contextual nature of names and locations. To solve this without neural networks, Conditional Random Fields (CRF) provide a statistical framework for Named Entity Recognition (NER). CRFs model the conditional probability of a label sequence y given an input sequence of tokens x by calculating the relationships between adjacent labels and token features.  
The Rust crfs crate, which provides bindings to the highly efficient CRFsuite, allows for the training of lightweight models that use token-level features such as word casing, suffixes, and the surrounding context (e.g., the previous and next words). A well-tuned CRF model can achieve competitive precision and recall for entities like "Person" or "Location" while requiring a fraction of the computational resources of a BERT-based model. Because CRF models are essentially transition matrices, their weights are extremely small (often sub-10MB), fitting perfectly within the 50MB binary size constraint.

### **Finite State Transducers and Fuzzy Matching**

To enhance recall for soft PII, Tier 1 can incorporate Finite State Transducers (FSTs) for fuzzy dictionary matching. FSTs allow the engine to identify PII even when there are minor typos or variations in spelling—a common occurrence in unstructured text. By representing a dictionary as a directed acyclic word graph, FSTs provide highly compressed storage and O(m) lookup time, where m is the length of the query string. This enables the redaction engine to cross-reference potential PII tokens against millions of entries in a multi-gigabyte dictionary that has been compressed into a few megabytes.

## **Tier 2: The Native-Inference Approach**

The Tier 2 strategy moves beyond purely algorithmic methods to incorporate lightweight neural inference using pure-Rust engines. The primary goal of this tier is to replace the ONNX Runtime (and its associated C++ baggage) with engines like tract or candle, enabling a "hermetic" build process that is native to the Rust toolchain.

### **Evaluating Tract and Candle for CLI Redaction**

The choice between Sonos' tract and Hugging Face's candle is a pivotal architectural decision. Sonos' tract is designed specifically for "tiny, no-nonsense" inference on the edge. It excels at optimizing neural networks for single-core CPU execution, making it ideal for a CLI tool like filegoblin. Tract's unique advantage is its ability to "cook" models: it can ingest an ONNX or TensorFlow model, perform constant folding and operator stripping, and export an optimized NNEF (Neural Network Exchange Format) graph that is significantly faster to load and execute than the original.  
In contrast, Hugging Face's candle focuses on a minimalist, PyTorch-like API and modern serialization formats like safetensors. Safetensors are designed for zero-copy data access, meaning the model weights are memory-mapped directly from the binary into the CPU's memory space without unnecessary allocations or copies. This significantly reduces the 50ms latency overhead during the tool's initialization phase, as the model is "ready" as soon as the file is mapped.

### **Deploying GLiNER-Tiny and BERT-Tiny**

To maintain the 50MB binary size, Tier 2 avoids large BERT-base models in favor of "tiny" variants. GLiNER-Tiny (Generalist Lightweight Named Entity Recognizer) is a particularly promising candidate for 2026\. GLiNER models use a bidirectional transformer architecture but are designed for zero-shot extraction, allowing the redaction engine to identify custom PII types by specifying labels at runtime. A quantized GLiNER-Tiny model (approximately 10M-25M parameters) can fit within 30MB while providing high recall for the 60+ PII categories commonly required in enterprise scenarios.  
Alternatively, BERT-Tiny or DistilBERT-Tiny models with 2 to 4 layers provide a robust baseline for soft PII. These models, when fine-tuned on PII-specific datasets like AI4Privacy, achieve F1 scores in the 85-90% range with weights that can be quantized to under 20MB. Using candle or tract, these models can be executed entirely on the CPU with minimal latency, satisfying the air-gapped compliance requirement without needing external GPU acceleration.

| Inference Engine | Memory Mapping | Static Linking Ease | Model Optimization | Best Use Case |
| :---- | :---- | :---- | :---- | :---- |
| **Tract** | NNEF-based | High (Pure Rust/No-std) | Strong (Operator Stripping) | CPU-heavy CLI/Edge |
| **Candle** | Safetensors (Zero-copy) | High (Minimalist) | Lazy Loading | Serverless / Rapid Startup |
| **ORT (Baseline)** | C++ Shared Libs | Low (Complex) | Generic ONNX | High-perf Server-side |

## **Tier 3: The Hybrid-Heuristic Approach**

The Tier 3 "Hybrid-Heuristic" approach represents the most advanced strategy for minimizing constant overhead. Instead of running a neural model over every byte of text, Tier 3 uses lightweight statistical triggers to "wake up" the more expensive neural passes only when necessary. This strategy is inspired by "Inference Set Design," where active learning principles are used to selectively obtain labels for the "hardest" examples while allowing "easy" examples to be handled by simpler rules.

### **Statistical Triggers and Entropy Analysis**

The first pass of a Tier 3 engine uses Shannon Entropy and Shannon-index calculations to identify segments of text that are likely to contain PII. Structured PII and certain soft PII (like addresses or complex organization names) often exhibit distinct entropy signatures compared to natural language prose. By analyzing the entropy of a sliding window of tokens, the engine can flag high-risk regions for deeper analysis.  
Another powerful trigger is the "Proximity Match Scoring Mechanism." This heuristic looks for "anchor words" (e.g., "Address:", "SSN:", "Contact") and calculates a score based on the proximity of potential PII tokens. If a numeric pattern is found within a specific distance of a "Should" or "Must" criterion anchor, the Tier 3 system triggers a neural pass from Tier 2 to confirm the entity type and boundary.

### **The Two-Pass Redaction Pipeline**

The Tier 3 architecture is implemented as a two-pass pipeline:

1. **Pass 1 (The Sentinel):** A high-speed Rust pass using iron\_safety or a similar regex-based tool. This pass handles 80-90% of common PII (emails, obvious SSNs) and calculates risk scores for every text segment.  
2. **Pass 2 (The Refiner):** The engine segments the remaining text and only sends "high-risk" windows to a quantized BERT-Tiny model running in tract.

This approach minimizes the constant overhead of model inference. For a standard 1,000-word document, Pass 1 might resolve 95% of the content in under 5ms, with Pass 2 only analyzing the remaining 5% of tokens that were flagged as ambiguous. This ensures that the engine stays well within the 50ms latency budget while maintaining the high recall (98%) observed in specialized PII classifiers like the Roblox model.

## **Feasibility Report: Overcoming Shared Library Hell**

A core objective of this research is to identify an implementation path that minimizes the "shared library hell" inherent in cross-platform static builds. For filegoblin to be truly portable, the binary must run on minimalist Linux environments, legacy Windows servers, and modern Apple silicon without requiring the installation of C++ runtimes or ONNX libraries.

### **Static Linking for Musl, MSVC, and Darwin**

Building for x86\_64-unknown-linux-musl is the most effective way to produce a statically linked binary that runs on any Linux distribution. However, developers must be wary of "native" dependencies that implicitly link against glibc. The Tier 2 and Tier 3 strategies, by favoring pure-Rust engines like tract or candle, largely avoid this issue. If native dependencies like OpenSSL are required for other filegoblin features, the "vendored" feature flag must be used to compile and statically link the library from source.  
On Windows-MSVC, static linking is achieved by ensuring that the Rust compiler is configured to use the static version of the Visual C++ Redistributable. On Apple-darwin, the challenge is different; while macOS does not support truly static binaries in the same way as Linux-musl, a "hermetic" binary can be achieved by bundling all dependencies within the Mach-O executable and ensuring no non-standard shared libraries are referenced.

### **Binary Size and Runtime Footprint**

The Tier 3 approach provides the best balance of binary size and accuracy. By using a Tier 1 sentinel pass (Regex/Aho-Corasick) and a Tier 2 refiner pass (Quantized BERT-Tiny), the total weight of the PII engine (logic \+ models) can be kept under 30MB. This leaves 20MB of the 50MB budget for filegoblin's core markdown conversion logic and other CLI utilities.

| Approach | Binary Size | F1 Score (Est) | Latency (Per 1KB) | Static Linking Ease |
| :---- | :---- | :---- | :---- | :---- |
| **Tier 1 (Algorithmic)** | \<10MB | 85% \- 90% | \<1ms | Excellent (Pure Rust) |
| **Tier 2 (Native Neural)** | 30MB \- 60MB | 93% \- 95% | 10ms \- 30ms | High (Rust Engines) |
| **Tier 3 (Hybrid)** | 35MB \- 65MB | 95%+ | 5ms \- 15ms | High (Rust Engines) |
| **Baseline (Distil-PII/ORT)** | \>150MB | 96% | 50ms+ | Low (C++ Deps) |

## **Ecosystem Audit: 2025/2026 Rust PII Tools**

The Rust ecosystem for PII discovery has evolved rapidly between 2024 and 2026\. The shift from experimental bindings to production-grade safety tools is evident in the new "Security" features on crates.io, which allow developers to audit their dependency tree for vulnerabilities in real-time.

### **Key Rust Crates for 2026**

* **redact-core v0.7.1:** A replacement for Microsoft Presidio written in Rust. It provides a core engine for PII detection and anonymization. While the redact-ner companion crate currently relies on ONNX Runtime, the core library is modular enough to support Tract or Candle backends.  
* **iron\_safety v0.3.0:** A specialized crate for real-time PII detection in multi-agent systems. It uses compiled regex patterns and is optimized for extremely low-latency scanning (\<100μs for 1KB). Its purpose is output sanitization, making it an ideal candidate for the final redaction layer in filegoblin.  
* **gline-rs:** This project has successfully ported GLiNER model inference to Rust. It mirrors the complex pre- and post-processing logic required for zero-shot NER, providing a roadmap for implementing high-recall soft PII discovery without a Python runtime.  
* **aho-corasick v1.x:** The bedrock of multi-pattern matching in Rust. Recent updates have focused on SIMD-accelerated prefilters and reduced memory state representations.

### **Sub-30MB "Tiny" Models**

* **bert-small-pii-detection:** A fine-tuned model for English PII. While the full safetensors file is 114MB, a 4-bit quantized version (available via Candle-compatible formats) can be reduced to approximately 28MB.  
* **GLiNER-Tiny (Quantized):** A specialized version of the generalist NER model. By stripping non-PII labels and using 8-bit quantization, the model weights can be compressed into a 22MB footprint.  
* **BERT-Tiny (2-layer):** Often used for sequence labeling in resource-constrained environments, these models offer the lowest latency for Tier 2 and Tier 3 architectures, with F1 scores remaining stable for high-frequency PII like names and dates.

## **Implementation Strategy and Future Outlook**

The technical analysis indicates that the "Northstar" goal for filegoblin—a 95%+ recall, sub-50ms latency PII engine in a 50MB binary—is achievable by transitioning from the ORT-based baseline to a Tier 3 Hybrid-Heuristic architecture. By utilizing the 2026 Rust ecosystem's advancements in native inference and security-audited crates, the project can resolve the "shared library hell" that currently plagues the Distil-PII/ORT lead.

### **Step 1: Algorithmic Foundation (Tier 1\)**

The first phase of development should focus on implementing the "Sentinel" pass using the aho-corasick and iron\_safety crates. This ensures that the most common PII (emails, SSNs, credit cards) is redacted with near-zero latency. By using FST-compressed dictionaries of common names and locations, the engine can achieve 85% recall for soft PII before any neural models are even loaded. This pass serves as the primary filter, calculating risk scores to determine if the document requires further analysis.

### **Step 2: Native Neural Integration (Tier 2\)**

The second phase involves integrating the Sonos tract engine. Tract's ability to "cook" models into optimized NNEF graphs is the key to maintaining a sub-50MB binary. The project should select a BERT-Tiny or GLiNER-Tiny model, quantize it to 4-bit precision, and strip all unnecessary operators. This model will be embedded into the filegoblin binary using include\_bytes\!, ensuring it is available in air-gapped environments without external file dependencies.

### **Step 3: Statistical Triggering (Tier 3\)**

The final phase involves the development of the heuristic triggering mechanism. By implementing entropy-based windowing and proximity scoring, the engine can intelligently route "hard" tokens to the Tract-based neural pass while allowing "easy" tokens to be handled by the Sentinel. This two-pass system effectively "averages out" the latency of the neural model, ensuring that the overall document processing time remains well under the 50ms limit.

### **Cross-Platform Compliance and Reliability**

To ensure target architecture compliance, the build pipeline must utilize cross or cargo-zigbuild to target x86\_64-unknown-linux-musl, Windows-msvc, and Apple-darwin. By adhering to a pure-Rust inference strategy, the project avoids the pitfalls of linking against different versions of glibc or dynamic C++ libraries across these platforms. The 2026 focus on "Trusted Publishing" and security advisories within crates.io further ensures that the "Privacy Shield" remains resistant to supply chain attacks, a critical requirement for a tool handling sensitive personal data.  
This architectural evolution from a generic transformer-based approach to a specialized, multi-tiered Rust system allows filegoblin to serve as a high-performance, secure, and truly portable CLI tool. The combination of deterministic algorithmic speed and lightweight neural accuracy provides a robust "Privacy Shield" that meets the regulatory and operational demands of the modern data landscape. Through meticulous model quantization, graph optimization via Tract, and a heuristic-first execution model, the technical constraints of binary size and latency are not merely met but mastered, positioning filegoblin as a leader in the next generation of privacy-preserving developer tools.

#### **Works cited**

1\. A hybrid rule-based NLP and machine learning approach for PII detection and anonymization in financial documents \- PMC, https://pmc.ncbi.nlm.nih.gov/articles/PMC12214779/ 2\. PII Anonymization with Small Language Models \- hoop.dev, https://hoop.dev/blog/pii-anonymization-with-small-language-models/ 3\. crates.io: development update \- Rust Blog, https://blog.rust-lang.org/2026/01/21/crates-io-development-update/ 4\. Program management update — January 2026 | Inside Rust Blog, https://blog.rust-lang.org/inside-rust/2026/02/11/program-management-update-2026-01/ 5\. Comparing Best NER Models For PII Identification \- Protecto AI, https://www.protecto.ai/blog/best-ner-models-for-pii-identification/ 6\. A Comparative Study of light-weight Language Models for PII Masking and their deployment for real conversational texts \- arXiv, https://arxiv.org/html/2512.18608v1 7\. pii \- Keywords \- crates.io: Rust Package Registry, https://crates.io/keywords/pii 8\. Comparing NER Models for PII Identification | by Protecto AI \- Medium, https://medium.com/@protectoai/comparing-ner-models-for-pii-identification-73cd4d96b891 9\. fbilhaut/gline-rs: Inference engine for GLiNER models, in Rust \- GitHub, https://github.com/fbilhaut/gline-rs 10\. pykeio/ort: Fast ML inference & training for ONNX models in Rust \- GitHub, https://github.com/pykeio/ort 11\. Build statically linked Rust binary with musl (and avoid a common pitfall) \- DEV Community, https://dev.to/abhishekpareek/build-statically-linked-rust-binary-with-musl-and-avoid-a-common-pitfall-ahc 12\. How to compile a static musl binary of a Rust project with native dependencies?, https://stackoverflow.com/questions/40695010/how-to-compile-a-static-musl-binary-of-a-rust-project-with-native-dependencies 13\. X86\_64-unknown-linux-unknown? \- language design \- Rust Internals, https://internals.rust-lang.org/t/x86-64-unknown-linux-unknown/15175 14\. Compile to statically linked binary like golang? \- The Rust Programming Language Forum, https://users.rust-lang.org/t/compile-to-statically-linked-binary-like-golang/5138 15\. model.safetensors · gravitee-io/bert-small-pii-detection at 98dbadfdd07dafd99ff09dc377fe8b9b3a71ee1d \- Hugging Face, https://huggingface.co/gravitee-io/bert-small-pii-detection/blob/98dbadfdd07dafd99ff09dc377fe8b9b3a71ee1d/model.safetensors 16\. aho\_corasick \- Rust \- Docs.rs, https://docs.rs/aho-corasick 17\. aho\_corasick \- Rust, http://nercury.github.io/twig-rs/aho\_corasick/index.html 18\. A fast implementation of Aho-Corasick in Rust. \- GitHub, https://github.com/BurntSushi/aho-corasick 19\. Stargix/NER-CRF: An nlp project to compare Conditional Random Fields model configurations on Named Entity Recognition \- GitHub, https://github.com/Stargix/NER-CRF 20\. crfs \- Rust \- Docs.rs, https://docs.rs/crfs 21\. huggingface/candle: Minimalist ML framework for Rust \- GitHub, https://github.com/huggingface/candle 22\. sonos/tract: Tiny, no-nonsense, self-contained, Tensorflow ... \- GitHub, https://github.com/sonos/tract 23\. Maximizing performance with tract · sonos tract · Discussion \#716 \- GitHub, https://github.com/sonos/tract/discussions/716 24\. SafeTensors: Efficient Serialization Format for Deep Learning | by Nishtha kukreti | Medium, https://medium.com/@nishthakukreti.01/safetensors-efficient-serialization-format-for-deep-learning-57364317be43 25\. Seeking Recommendations for Best Small Model to Detect PII in Text \- Reddit, https://www.reddit.com/r/LocalLLaMA/comments/1eckcoe/seeking\_recommendations\_for\_best\_small\_model\_to/ 26\. Transformer Models for PII Detection \- Kaggle, https://www.kaggle.com/code/gpreda/transformer-models-for-pii-detection 27\. How to Load Embedding Models like BERT using Candle Crate in Rust \- Medium, https://medium.com/@kamaljp/how-to-load-embedding-models-like-bert-using-candle-crate-in-rust-dada119f08c9 28\. Lightweight CPU-Only PII Detection AI Model for Fast, Secure Data Processing \- hoop.dev, https://hoop.dev/blog/lightweight-cpu-only-pii-detection-ai-model-for-fast-secure-data-processing/ 29\. Efficient Biological Data Acquisition through Inference Set Design \- arXiv, https://arxiv.org/html/2410.19631v3 30\. Multivariable Heuristic Approach to Intrusion Detection in Network Environments \- MDPI, https://www.mdpi.com/1099-4300/23/6/776 31\. Decoding Complexity: CHPDA – Intelligent Pattern Exploration with a Context-Aware Hybrid Pattern Detection Algorithm \- arXiv, https://arxiv.org/html/2502.07815v1 32\. iron\_safety \- Rust \- Docs.rs, https://docs.rs/iron\_safety 33\. The Holy Grail of Linux Binary Compatibility: Musl and Dlopen | Hacker News, https://news.ycombinator.com/item?id=46762882 34\. Build a Statically Linked or Mostly-Statically Linked Native Executable \- GraalVM, https://www.graalvm.org/latest/reference-manual/native-image/guides/build-static-executables/ 35\. redact-ner \- Rust Package Registry \- Crates.io, https://crates.io/crates/redact-ner/versions 36\. gliner \- Rust \- Docs.rs, https://docs.rs/gline-rs/