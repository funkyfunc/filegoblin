# **Architectural Specification for the Privacy Shield: A Tiered Hybrid-Heuristic Redaction Engine**

The development of filegoblin, a high-performance command-line interface (CLI) tool, necessitates a rigorous approach to data privacy that transcends traditional pattern-matching techniques. The primary objective is the implementation of the "Privacy Shield," a redaction engine capable of identifying and obscuring Personal Identifiable Information (PII) with a recall rate exceeding 95% while maintaining a processing latency of less than 50 milliseconds for a standard 1,000-word document. This technical specification outlines a Tier 3 Hybrid-Heuristic architecture designed to operate within the strict constraints of a hermetic, zero-dependency Rust binary under 50 megabytes. The architecture leverages 2026-native technologies, prioritizing pure-Rust inference and optimized algorithmic passes to ensure cross-platform compatibility across Linux (musl), Windows (MSVC), and Apple Darwin environments.

## **Tier 1: The Sentinel Algorithmic Pass**

The foundational layer of the Privacy Shield, designated as the Sentinel, serves as the primary filter for structured and high-confidence PII. By addressing approximately 80% to 90% of PII candidates in a single linear scan, the Sentinel minimizes the computational burden on subsequent neural layers. This pass integrates multi-pattern search automata and compressed dictionaries to achieve near-zero latency overhead.

### **Multi-Pattern Automata with Aho-Corasick**

The core of the Sentinel's efficiency lies in the Aho-Corasick algorithm, a finite state machine (FSM) construction that enables the simultaneous search of multiple patterns in ![][image1] time, where ![][image2] represents the input text length and ![][image3] is the cumulative length of the patterns.1 For the filegoblin engine, the aho-corasick crate provides a robust implementation that utilizes SIMD (Single Instruction, Multiple Data) acceleration for prefiltering.3

The implementation of the automaton must carefully select a transition strategy to balance the 50MB binary size constraint against the sub-50ms latency target. The choice between Dense, Sparse, and Full automata significantly impacts the performance profile of the Sentinel pass.

| Automaton Strategy | Memory Footprint | Search Throughput | Contextual Applicability |
| :---- | :---- | :---- | :---- |
| Sparse | Minimal (Compressed transitions) | Moderate | Large pattern sets in memory-constrained environments.5 |
| Dense | Moderate (256-slot map per state) | High | Balanced performance for standard PII patterns.5 |
| Full DFA | High (Pre-computed transitions) | Maximum | Fixed, high-frequency PII tokens where binary size permits.2 |

To ensure maximum recall, the engine utilizes the LeftmostFirst match kind. This prevents the "Sam" vs. "Samwise" problem, where an standard Aho-Corasick implementation might report a match for "Sam" and exit, potentially leaving sensitive suffixes unredacted.2 By prioritizing the longest possible match at any given position, the Sentinel ensures that structured identifiers like complex account numbers or full names are captured in their entirety.2

### **SIMD-Accelerated Prefilters and the Packed Sub-module**

High-performance searching in 2026-native Rust is further augmented by vectorized routines. The aho-corasick library internally employs prefilters that use SIMD instructions to quickly skip sections of the haystack that cannot possibly contain a match.3 These prefilters are most effective when the number of patterns is relatively small (typically under 100), making them ideal for high-risk PII keywords such as "SSN", "Passport", or "Credit Card".3

For specific high-bandwidth scenarios, the packed sub-module of the aho-corasick crate allows for side-stepping the full automaton for a subset of critical patterns. This lower-level API executes vectorized multiple substring searches that can achieve throughputs several times higher than the standard FSM, provided the number of patterns remains constrained.3

### **FST-Compressed Dictionaries for Soft PII**

While regular expressions effectively capture structured PII like emails or social security numbers, "soft" PII such as names, organizations, and geographic locations requires the use of massive look-up tables. To keep the filegoblin binary under 50MB, the Privacy Shield utilizes Finite State Transducers (FST). FSTs are highly compressed data structures that represent ordered sets or maps, enabling lookups in time proportional to the length of the key rather than the total size of the dictionary.6

The fst crate permits the storage of billions of keys in a format that remains searchable without full decompression.6 This is critical for the "air-gapped" constraint of filegoblin, as the dictionary can be memory-mapped directly from the binary's data segment, ensuring no disk I/O occurs during model loading or execution.6 Furthermore, the fst implementation supports fuzzy querying through Levenshtein automata, allowing the engine to catch variations in name spellings or minor typographical errors that might otherwise bypass a strict string-matching filter.6

### **Integration with iron\_safety**

The Sentinel pass is complemented by the iron\_safety framework, which provides real-time PII detection and output sanitization.8 The PiiDetector within this crate uses pre-compiled regex patterns optimized for performance, with a check operation typically requiring less than 100 microseconds per kilobyte of text.8 This integration is particularly useful for handling streaming data, where chunks are checked frequently to prevent the accumulation of large buffers.8

## **Tier 2: The Refiner Native Inference**

The second tier, designated as the Refiner, is a neural inference pass designed to classify ambiguous tokens that the Sentinel pass cannot definitively resolve. In a hybrid architecture, the Refiner is only activated when the heuristic "Trigger" (Tier 3\) identifies a high-risk text window. This tier replaces the traditionally heavy ONNX Runtime with pure-Rust engines to minimize binary size and eliminate external system dependencies.

### **Transitioning to Pure-Rust Engines: Tract and Candle**

To satisfy the 50MB binary UX requirement, filegoblin avoids the inclusion of the C++ based ONNX Runtime. Instead, it evaluates two primary contenders: Tract and Candle. Tract, developed by Sonos, is designed for embedding neural network inference on edge devices.10 It is particularly effective at "cooking" models—a process of optimizing and serializing the computation graph into a format that is faster to load and execute on specific hardware.10

| Engine | Architecture | Weight Format | Zero-Copy Support |
| :---- | :---- | :---- | :---- |
| Tract | Pure Rust | NNEF / OPL | High (via OPL/NNEF) 10 |
| Candle | Pure Rust | Safetensors / GGUF | Maximum (via Safetensors) 12 |

Tract's strength lies in its ability to convert models into the Neural Network Exchange Format (NNEF), an industry-standard format focused on inference rather than training.13 The NNEF format strips away training-specific information, resulting in assets that are human-readable, easily extensible, and highly optimized for on-device machine learning.13

### **The "Cooking" Workflow for NNEF Graphs**

The process of "cooking" a model for the filegoblin binary involves several discrete optimization steps within the Tract ecosystem. First, the model is loaded from its source format (e.g., ONNX) and "decluttered"—a procedure that removes redundant operators and simplifies the graph.15 Following decluttering, the model is transformed into a "typed" model and subsequently into an NNEF archive.11

This conversion process significantly reduces load times. In a gaming or real-time CLI context, pre-cooked NNEF models have demonstrated loading times as low as sub-10ms, a stark improvement over the 1500ms often required for standard ONNX loading in web or embedded environments.11 By using the tract\_nnef crate, the engine can load these pre-optimized graphs directly from memory using include\_bytes\!, adhering to the air-gapped and hermetic binary constraints.15

### **Safetensors and Zero-Copy Memory Mapping**

For models that utilize the Candle framework, the safetensors format provides an alternative path to high-performance inference. Safetensors is a format for storing tensors that is safe, fast, and allows for zero-copy loading.12 This is achieved by memory-mapping the file (or the embedded binary blob), allowing the engine to access weights without allocating new memory.18

The use of MmapedSafetensors in the candle-core crate ensures that the model weights do not consume excess RAM beyond the initial binary footprint.18 In the context of a 50MB binary, where model weights may consume 30-40MB, zero-copy loading is essential to prevent the engine from exceeding system memory limits during execution. This approach also eliminates the security risks associated with the Python pickle format, which is prone to arbitrary code execution.12

### **Quantization and Binary Size Management**

To fit a sophisticated PII classification model like GLiNER-Tiny or BERT-Tiny into the 50MB limit, quantization is a necessity. Quantization reduces the precision of model weights (e.g., from 32-bit floating point to 8-bit or 4-bit integers), which significantly decreases the model's storage requirements without a proportional loss in accuracy.12

Candle supports quantization techniques similar to those used in llama.cpp, including GGUF support for quantized models.12 Similarly, Tract enables the export of quantized networks with 4 bits or less, allowing for deeper integration and smaller binary sizes.14 By leveraging these techniques, the Refiner can maintain a 95%+ recall rate while occupying a minimal portion of the 50MB binary budget.

## **Tier 3: The Trigger Heuristic Logic**

The Trigger is the architectural component responsible for bridging the high-speed Sentinel (Tier 1\) and the context-aware Refiner (Tier 2). Its primary function is to analyze text windows in real-time and determine whether the neural pass should be "woken up." This is achieved through specific mathematical heuristics that measure the probability of PII presence within a given window.

### **Shannon Entropy for High-Risk Detection**

The primary heuristic employed by the Trigger is Shannon Entropy. This metric provides a mathematical measure of the uncertainty or randomness in a data stream.21 The entropy ![][image4] of a discrete random variable ![][image5] is defined as:

![][image6]  
where ![][image7] is the probability of occurrence of the ![][image8]\-th symbol in the sliding window.22 In the context of PII redaction, high entropy often signals the presence of non-natural language tokens, such as encrypted keys, hashed identifiers, or structured alphanumeric codes that are characteristic of sensitive data.21

The Trigger implements this through a sliding window approach. As the engine scans the text, it maintains a frequency count of characters within the current window.22 This allows the entropy to be calculated in ![][image9] time, where ![][image10] is the window size.22 If the entropy exceeds a pre-calculated threshold (determined by analyzing the entropy profiles of common PII types), the Refiner is triggered for that specific segment.21

### **Proximity Scoring and Window Management**

In addition to entropy, the Trigger utilizes proximity scoring to identify clusters of potential PII. Many PII tokens do not appear in isolation; for example, a name is frequently preceded by titles (Mr., Ms., Dr.) or context-specific anchors ("Name:", "Attn:"). The Trigger monitors the proximity of these anchor words—identified during the Sentinel pass—to unknown capitalized tokens or numeric sequences.

A "risk score" is calculated for each window, combining the entropy value with proximity weights. If the aggregate score crosses a dynamic threshold, the window is passed to the Refiner. This heuristic routing ensures that the sub-50ms latency target is met by only running the expensive neural inference on a small fraction (e.g., \<5%) of the total document text.

## **Technical Implementation and Dependency Audit**

The implementation of the Privacy Shield requires a carefully curated set of Rust crates that are compatible with the 2026-native requirements of the project. The following dependency audit identifies the necessary versions and features to ensure a zero-dependency, statically linked executable.

### **Cargo.toml Specification**

The following Cargo.toml snippet outlines the crate requirements for the Tier 3 architecture, ensuring all features are geared toward performance and static linking.

Ini, TOML

\[package\]  
name \= "filegoblin"  
version \= "0.1.0"  
edition \= "2021"

\[dependencies\]  
\# Tier 1: Sentinel Pass  
aho-corasick \= { version \= "1.1", features \= \["perf-literal", "std"\] } \# SIMD acceleration  
fst \= { version \= "0.4", features \= \["levenshtein"\] } \# Compressed dictionaries  
iron\_safety \= { version \= "0.3", default-features \= false, features \= \["enabled"\] }

\# Tier 2: Refiner Pass  
tract-nnef \= "0.22" \# For cooked NNEF models  
tract-onnx \= "0.22" \# For initial model loading/cooking  
candle-core \= { version \= "0.9", features \= \["mkl"\] } \# Minimalist ML with MKL support  
safetensors \= "0.4" \# Zero-copy loading

\# Tier 3: Trigger & Utilities  
sliding\_features \= "7.1" \# Modular sliding window functions  
memmap2 \= "0.9" \# Cross-platform memory mapping  
serde \= { version \= "1.0", features \= \["derive"\] }  
anyhow \= "1.0"

\[build-dependencies\]  
cc \= "1.0" \# For static linking of C-based dependencies

### **The Trigger Algorithm Implementation**

The following Rust code example demonstrates a sliding-window entropy scanner that identifies "high-risk" tokens and routes them to a neural refiner. This implementation uses a frequency-based probability calculation within a fixed window.

Rust

use std::collections::HashMap;

pub struct PiiTrigger {  
    window\_size: usize,  
    entropy\_threshold: f64,  
}

impl PiiTrigger {  
    pub fn new(window\_size: usize, threshold: f64) \-\> Self {  
        Self { window\_size, entropy\_threshold: threshold }  
    }

    /// Calculates the Shannon Entropy of a given byte slice.  
    fn calculate\_entropy(&self, data: &\[u8\]) \-\> f64 {  
        let mut counts \= HashMap::new();  
        for \&byte in data {  
            \*counts.entry(byte).or\_insert(0) \+= 1;  
        }

        let len \= data.len() as f64;  
        let mut entropy \= 0.0;

        for \&count in counts.values() {  
            let p \= count as f64 / len;  
            entropy \-= p \* p.log2();  
        }

        entropy  
    }

    /// Scans text and returns window indices that exceed the entropy threshold.  
    pub fn scan(&self, text: &str) \-\> Vec\<(usize, usize)\> {  
        let bytes \= text.as\_bytes();  
        let mut trigger\_points \= Vec::new();

        if bytes.len() \< self.window\_size {  
            return trigger\_points;  
        }

        for i in 0..=(bytes.len() \- self.window\_size) {  
            let window \= \&bytes\[i..i \+ self.window\_size\];  
            let entropy \= self.calculate\_entropy(window);

            if entropy \> self.entropy\_threshold {  
                trigger\_points.push((i, i \+ self.window\_size));  
            }  
        }

        trigger\_points  
    }  
}

This algorithm achieves ![][image11] complexity in its simplest form, where ![][image2] is text length and ![][image10] is window size. For production, the entropy calculation can be further optimized by incrementally updating the frequency counts as the window slides, reducing the per-step complexity to ![][image12].22

## **Model Optimization and Static Build Workflow**

Meeting the 50MB binary and air-gapped constraints requires a specialized build pipeline that transforms high-level models into static, embedded assets.

### **Model "Cooking" for Tract**

To convert an ONNX model (such as GLiNER-Tiny) into an optimized NNEF graph for filegoblin, the following CLI workflow is utilized.

1. **Load and Declutter**: The ONNX model is loaded and processed to remove training-specific artifacts.  
   Bash  
   tract model.onnx \--input-bundle input.npz \-O dump \--nnef-tar model.nnef.tar

2. **Quantization**: Utilizing torch-to-nnef, the model can be further quantized to 8-bit or 4-bit precision.14  
   Python  
   from torch\_to\_nnef import export\_model\_to\_nnef, TractNNEF  
   export\_model\_to\_nnef(model, args=tuple(inputs), file\_path\_export="model.nnef.tgz",   
                        inference\_target=TractNNEF(version="0.21.13"))

3. **Embedding**: The resulting .tar or .tgz archive is embedded into the Rust binary.  
   Rust  
   const MODEL\_DATA: &\[u8\] \= include\_bytes\!("../assets/model.nnef.tar");

### **Static Build Configuration for musl, MSVC, and Darwin**

Ensuring a zero-dependency binary across platforms requires specific compiler flags and the management of native libraries.

For Linux, the target x86\_64-unknown-linux-musl is used to create a 100% statically linked binary.26 A common pitfall occurs when dependencies like OpenSSL are linked dynamically. This is resolved by enabling the vendored feature for such crates.26

| Target | Toolchain | Static Flag / Configuration |
| :---- | :---- | :---- |
| Linux (musl) | musl-gcc | \-C target-feature=+crt-static 28 |
| Windows | MSVC | \["-C", "target-feature=+crt-static"\] in .cargo/config 29 |
| Apple Darwin | Xcode/Clang | Standard static linking (limited by OS) 29 |

The build.rs script must communicate these linking options to Cargo. For example, to force static linking of a native library, the script should output:

Rust

fn main() {  
    println\!("cargo:rustc-link-lib=static=ssl");  
    println\!("cargo:rustc-link-lib=static=crypto");  
}

Furthermore, using an alternative allocator like mimalloc can significantly improve performance for musl-based binaries, which often suffer from thread congestion in the default allocator.30

## **Benchmarking and Performance Verification**

To ensure the sub-50ms latency for a 1,000-word document, a rigorous benchmarking suite using Criterion.rs is implemented. Criterion provides statistically driven micro-benchmarking with confidence intervals to detect even minor performance regressions.31

### **Criterion.rs Benchmark Template**

The following template measures the throughput of the Privacy Shield engine, reporting both time per iteration and bytes per second.

Rust

use criterion::{black\_box, criterion\_group, criterion\_main, Criterion, Throughput};  
use filegoblin::PrivacyShield;

fn benchmark\_redaction\_throughput(c: &mut Criterion) {  
    let mut shield \= PrivacyShield::init(); // Tiered engine initialization  
    let doc\_1000\_words \= include\_str\!("../tests/samples/document\_1k.txt");  
      
    let mut group \= c.benchmark\_group("privacy-shield");  
    group.throughput(Throughput::Bytes(doc\_1000\_words.len() as u64));  
      
    group.bench\_function("redact\_1k\_words", |b| {  
        b.iter(|| shield.redact(black\_box(doc\_1000\_words)))  
    });  
      
    group.finish();  
}

criterion\_group\!(benches, benchmark\_redaction\_throughput);  
criterion\_main\!(benches);

By configuring the sample\_size and significance\_level, the developers can ensure that the performance measurements are robust against noise and outliers.33 The benchmark reports should show a linear regression profile with a high ![][image13] value, indicating consistent performance across varying input sizes.34

## **Binary UX and Size Optimization**

The 50MB binary constraint is a "hard" technical limit that influences every architectural decision. To achieve this, the following optimization strategies are applied during the final build phase.

### **Link-Time Optimization (LTO) and Code Generation**

Enabling "fat" LTO and setting codegen-units \= 1 in the release profile allows the compiler to perform global optimizations across crate boundaries.35 This results in smaller, more efficient binaries by eliminating unused code paths and performing aggressive inlining.

### **Symbol Stripping**

The final executable size is further reduced by stripping debug symbols. This can be performed either through the strip \= true option in Cargo.toml or manually via the strip command on Linux/macOS.27 For filegoblin, this typically yields a 15-20% reduction in final binary size.

| Optimization Technique | Binary Size Impact | Performance Impact |
| :---- | :---- | :---- |
| Fat LTO | \-10% | \+5% Speed |
| Symbol Stripping | \-15% | None |
| Quantization (8-bit) | \-50% (of model) | \+20% Speed |
| NNEF Cooking | \-5% | \+30% Load Speed |

## **Air-Gapped Compliance and In-Memory Execution**

To satisfy the air-gapped compliance requirement, all PII processing must occur in-memory after the initial binary execution. This means no temporary files are created, and no external calls are made to model registries or remote APIs.30

### **In-Memory Model Loading**

By using the include\_bytes\! macro, model weights and FST dictionaries are compiled directly into the binary's .rodata section. At runtime, the engine uses safe pointers to access this data without copying it into the heap, maintaining a low memory footprint.38

### **Zero-IO Operation**

The entire Tier 3 architecture is designed as a pure-data-in, data-out system. The Sentinel, Refiner, and Trigger components operate on slices (&\[u8\] or \&str) and return modified strings or indices.5 This ensures that the engine can be used in highly sensitive environments, such as processing logs within a secure enclave or a restricted container, where any disk I/O would trigger security alerts or compromise data isolation.9

## **Conclusion and Future Outlook**

The Tier 3 Hybrid-Heuristic architecture for the Privacy Shield engine represents a state-of-the-art approach to PII redaction in a local-first, high-performance context. By combining the linear-time speed of the Aho-Corasick Sentinel pass with the context-aware precision of the NNEF-optimized Refiner and the intelligent gating of the Shannon Entropy Trigger, filegoblin achieves its ambitious recall and latency targets.

The use of pure-Rust inference engines and the safetensors format ensures that the tool remains a hermetic, zero-dependency binary, capable of cross-platform execution without "shared library hell." As the field of on-device machine learning continues to evolve through 2026, the modular nature of this architecture allows for the seamless integration of newer, more efficient models and hardware-accelerated kernels as they become available in the Rust ecosystem. This technical specification provides the roadmap for building a redaction engine that is not only powerful and efficient but also deeply aligned with the modern requirements of data privacy and security.

#### **Works cited**

1. Package AhoCorasickTrie \- CRAN \- R-project.org, accessed February 22, 2026, [https://cran.r-project.org/package=AhoCorasickTrie](https://cran.r-project.org/package=AhoCorasickTrie)  
2. A fast implementation of Aho-Corasick in Rust. \- GitHub, accessed February 22, 2026, [https://github.com/BurntSushi/aho-corasick](https://github.com/BurntSushi/aho-corasick)  
3. aho\_corasick \- Rust \- Docs.rs, accessed February 22, 2026, [https://docs.rs/aho-corasick](https://docs.rs/aho-corasick)  
4. aho\_corasick \- Rust \- Docs.rs, accessed February 22, 2026, [https://docs.rs/aho-corasick/latest/aho\_corasick/index.html\#prefilters](https://docs.rs/aho-corasick/latest/aho_corasick/index.html#prefilters)  
5. aho\_corasick \- Rust, accessed February 22, 2026, [http://nercury.github.io/twig-rs/aho\_corasick/index.html](http://nercury.github.io/twig-rs/aho_corasick/index.html)  
6. fst \- Rust \- Docs.rs, accessed February 22, 2026, [https://docs.rs/fst/latest/fst/](https://docs.rs/fst/latest/fst/)  
7. fst \- crates.io: Rust Package Registry, accessed February 22, 2026, [https://crates.io/crates/fst](https://crates.io/crates/fst)  
8. iron\_safety \- Rust \- Docs.rs, accessed February 22, 2026, [https://docs.rs/iron\_safety](https://docs.rs/iron_safety)  
9. iron\_runtime \- Rust \- Docs.rs, accessed February 22, 2026, [https://docs.rs/iron\_runtime](https://docs.rs/iron_runtime)  
10. tract/doc/intro.md at main · sonos/tract \- GitHub, accessed February 22, 2026, [https://github.com/sonos/tract/blob/main/doc/intro.md](https://github.com/sonos/tract/blob/main/doc/intro.md)  
11. Maximizing performance with tract · sonos tract · Discussion \#716 \- GitHub, accessed February 22, 2026, [https://github.com/sonos/tract/discussions/716](https://github.com/sonos/tract/discussions/716)  
12. huggingface/candle: Minimalist ML framework for Rust \- GitHub, accessed February 22, 2026, [https://github.com/huggingface/candle](https://github.com/huggingface/candle)  
13. tract-nnef \- Lib.rs, accessed February 22, 2026, [https://lib.rs/crates/tract-nnef](https://lib.rs/crates/tract-nnef)  
14. Shipping neural networks with Torch to NNEF \- Tech Blog \- Sonos, accessed February 22, 2026, [https://tech-blog.sonos.com/posts/torch-2-nnef-open-sourcing/](https://tech-blog.sonos.com/posts/torch-2-nnef-open-sourcing/)  
15. Saving tract\_onnx model · sonos tract · Discussion \#1094 · GitHub, accessed February 22, 2026, [https://github.com/sonos/tract/discussions/1094](https://github.com/sonos/tract/discussions/1094)  
16. Creating executable bundle for ONNX model · Issue \#393 · sonos/tract \- GitHub, accessed February 22, 2026, [https://github.com/sonos/tract/issues/393](https://github.com/sonos/tract/issues/393)  
17. tract \- Rust \- Docs.rs, accessed February 22, 2026, [https://docs.rs/tract-ffi/latest/tract/](https://docs.rs/tract-ffi/latest/tract/)  
18. candle\_core::safetensors \- Rust \- Docs.rs, accessed February 22, 2026, [https://docs.rs/candle-core/latest/candle\_core/safetensors/index.html](https://docs.rs/candle-core/latest/candle_core/safetensors/index.html)  
19. Machine learning — list of Rust libraries/crates // Lib.rs, accessed February 22, 2026, [https://lib.rs/science/ml](https://lib.rs/science/ml)  
20. tract torch-to-nnef, accessed February 22, 2026, [https://fosdem.org/2026/events/attachments/YJJQTD-tract-and-torch-to-nnef/slides/266716/tract-and\_l9xkrho.pdf](https://fosdem.org/2026/events/attachments/YJJQTD-tract-and-torch-to-nnef/slides/266716/tract-and_l9xkrho.pdf)  
21. On the Application of Entropy Measures with Sliding Window for Intrusion Detection in Automotive In-Vehicle Networks \- MDPI, accessed February 22, 2026, [https://www.mdpi.com/1099-4300/22/9/1044](https://www.mdpi.com/1099-4300/22/9/1044)  
22. CUDA Calculation of Shannon Entropy for a Sliding Window System \- Repository of UKIM, accessed February 22, 2026, [https://repository.ukim.mk/bitstream/20.500.12188/32232/1/CUDA%20Calculation%20of%20Shannon%20Entropy%20for%20a%20Sliding%20Window%20System%20-%20accepted%20version.pdf](https://repository.ukim.mk/bitstream/20.500.12188/32232/1/CUDA%20Calculation%20of%20Shannon%20Entropy%20for%20a%20Sliding%20Window%20System%20-%20accepted%20version.pdf)  
23. Detection of Cybersecurity Events Based on Entropy Analysis \- CEUR-WS.org, accessed February 22, 2026, [https://ceur-ws.org/Vol-3382/Paper21.pdf](https://ceur-ws.org/Vol-3382/Paper21.pdf)  
24. Sliding Window Optimized Information Entropy Analysis Method for Intrusion Detection on In-Vehicle Networks \- Computer Science, accessed February 22, 2026, [https://cs.newpaltz.edu/\~lik/publications/Wufei-Wu-IEEE-Access-2018.pdf](https://cs.newpaltz.edu/~lik/publications/Wufei-Wu-IEEE-Access-2018.pdf)  
25. Mastering the Sliding Window Technique: A Comprehensive Guide | by Nikhil Bajpai, accessed February 22, 2026, [https://medium.com/@nikhil.cse16/mastering-the-sliding-window-technique-a-comprehensive-guide-6bb5e1e86f99](https://medium.com/@nikhil.cse16/mastering-the-sliding-window-technique-a-comprehensive-guide-6bb5e1e86f99)  
26. How to compile a static musl binary of a Rust project with native dependencies?, accessed February 22, 2026, [https://stackoverflow.com/questions/40695010/how-to-compile-a-static-musl-binary-of-a-rust-project-with-native-dependencies](https://stackoverflow.com/questions/40695010/how-to-compile-a-static-musl-binary-of-a-rust-project-with-native-dependencies)  
27. Build statically linked Rust binary with musl (and avoid a common pitfall) \- DEV Community, accessed February 22, 2026, [https://dev.to/abhishekpareek/build-statically-linked-rust-binary-with-musl-and-avoid-a-common-pitfall-ahc](https://dev.to/abhishekpareek/build-statically-linked-rust-binary-with-musl-and-avoid-a-common-pitfall-ahc)  
28. Static Build of Rust Executables | Ivanovo, accessed February 22, 2026, [https://zderadicka.eu/static-build-of-rust-executables/](https://zderadicka.eu/static-build-of-rust-executables/)  
29. rust \- How to generate statically linked executables? \- Stack Overflow, accessed February 22, 2026, [https://stackoverflow.com/questions/31770604/how-to-generate-statically-linked-executables](https://stackoverflow.com/questions/31770604/how-to-generate-statically-linked-executables)  
30. Static linking for rust without glibc \- scratch image, accessed February 22, 2026, [https://users.rust-lang.org/t/static-linking-for-rust-without-glibc-scratch-image/112279](https://users.rust-lang.org/t/static-linking-for-rust-without-glibc-scratch-image/112279)  
31. criterion \- Rust \- Docs.rs, accessed February 22, 2026, [https://docs.rs/criterion/latest/criterion/](https://docs.rs/criterion/latest/criterion/)  
32. Rust Benchmarking with Criterion.rs \- Rustfinity, accessed February 22, 2026, [https://www.rustfinity.com/blog/rust-benchmarking-with-criterion](https://www.rustfinity.com/blog/rust-benchmarking-with-criterion)  
33. Advanced Configuration \- Criterion.rs Documentation, accessed February 22, 2026, [https://bheisler.github.io/criterion.rs/book/user\_guide/advanced\_configuration.html](https://bheisler.github.io/criterion.rs/book/user_guide/advanced_configuration.html)  
34. Command-Line Output \- Criterion.rs Documentation, accessed February 22, 2026, [https://bheisler.github.io/criterion.rs/book/user\_guide/command\_line\_output.html](https://bheisler.github.io/criterion.rs/book/user_guide/command_line_output.html)  
35. Is there a production ready Trie crate? \- help \- The Rust Programming Language Forum, accessed February 22, 2026, [https://users.rust-lang.org/t/is-there-a-production-ready-trie-crate/24208](https://users.rust-lang.org/t/is-there-a-production-ready-trie-crate/24208)  
36. Building Rust binaries for different platforms, accessed February 22, 2026, [https://takashiidobe.com/gen/building-rust-binaries-for-different-platforms](https://takashiidobe.com/gen/building-rust-binaries-for-different-platforms)  
37. Machine Learning-Based Vulnerability Detection in Rust Code Using LLVM IR and Transformer Model \- MDPI, accessed February 22, 2026, [https://www.mdpi.com/2504-4990/7/3/79](https://www.mdpi.com/2504-4990/7/3/79)  
38. candle\_core/ safetensors.rs, accessed February 22, 2026, [https://docs.rs/candle-core/latest/src/candle\_core/safetensors.rs.html](https://docs.rs/candle-core/latest/src/candle_core/safetensors.rs.html)  
39. candle/candle-core/src/safetensors.rs at main \- GitHub, accessed February 22, 2026, [https://github.com/huggingface/candle/blob/main/candle-core/src/safetensors.rs](https://github.com/huggingface/candle/blob/main/candle-core/src/safetensors.rs)  
40. iron\_runtime \- crates.io: Rust Package Registry, accessed February 22, 2026, [https://crates.io/crates/iron\_runtime](https://crates.io/crates/iron_runtime)

[image1]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAF0AAAAYCAYAAACY5PEcAAAEkUlEQVR4Xu2YaahVVRTHV4PZYGUTpVbPwtCKvjSnopEEDSQNWiSVzQNBRR+iKPJhQgmK0YAgEs3jl0gI1D5UVhYNNEM2SNHch6JoLvP/c53j3Wfdc8593nffI7j3B3/uOWvtc+4+6+y99jrbrEePHu2xm7RDNHYZxGDHaBwoI6Vp0qnSmOArY6z0mrRrdHQZh0gvSTtFRx0TpWekD6V+6XbpZ2mltHejWQFe0KvSRdEhdpE+kr6WvjW/7/aFFmZ3SZ9LH0ufSq8U3cPCttK70jfm/fxKGldoUQTfZ+ZtebZ3rBHoxeYx3Do7r+Um85tcYN6JnD7pdWm9NCqx5ywy928VHQlXSj9J/1n5y5lh/t9HWv19hhr6xgCgn8cXXQWWmff3O2tOqQw07JcFexNLpD+lo6Mj40LzjswLdv6AmTA52COPS3PN7/FW8MFJ0r3ROEB4SY9EY5sslRZY9eCAmdIN0gbpseDLuVj6QtouOnJON/+TuocmZ9PmjWC/Vno/2Mpg+hEcUgf3iS+JB50dbAOFWbkqGtuEAXGoeR9vCz5gpj8ozTJvc3nRvZltpL+kM6IDWPjIX7+bB7aKnc3f7I/Bvlp6OtgirBNPZsfnmnf20YZ7Ey9KewXbQBlhnQk6//+C+f3+kR4uujdxh/nz3G3+HAcV3QXWSfdEI1xvfjE5qg5GJu1Y6FI+Mc/pdVwhXZUd80AsPIyCfTIbOfHN7LgdmMKdCPpZ0vzsmLz+cuKDw6VbsuP3zAdrHc+az/AmnjMP5qXRESBwtFuR2PIpxCJZB3nv4OScdYF73Zqds4gyctqlU0GnDydkx4x4KpkcnpWRT6XGjGDWt1pHqMp+iEZGHYsnATgm+CIEm3bXJba+zEbQ6ng7nDPCeVmMFPpAPj+z0KKa3c3TYKrx5kGKdrQlHyrMtrwSecA8sPk5a9f07DjP562qk2ukfy2Ujrw1bozKSsEcFhbafGnFGpvpxp/zW8Uka+TzFEYJ154jrZH2KLorWWg+c1I9YV6+RTs62y9rCaOXdSWn37x/zND9pTsTH3m6VT6H883j1lTBkKO5AaOiCt46bbhJygGZnTdfBbn86mgUx5lfSzUUK6ItpRPphcqJGZcz17x/p0j3mc+wnA+sdT4H0mdpO8ofbl41IqhVWcmZKhEqGq69MToSnpIOi8YMgs316Shqh04Enfr8xOScLRD69rw0J7HzVY49Vl9lEFu2BJpgX4UycK01T/FLpL+tPneRcpZHYwbB5t5V2wf5aOI7YTAMNugTzNMT+yY5+5r3je2PFGY79lbFA/BNQuBLOcp834HyjxFN3lxn/iBTk3Zl3G8+GlJGm+9LUPujX61RaqWwplCasTM3GNoNOos4qeI3837+Yf5lDnzIrZcOzM6p7kjFfH3Tli0N9oriFkDK9+ZfrpXwVXeEdJ75zmJf0V0J+ZyyKG5iDSftBn0o2c+8Msy/RToK5RA7hzdHxzDCqGQ/5P8E1VnZF23HOE36xeq3QruJKeZrWauSctCQsx+Kxi6Emc8298nRMVT0S3tGY5dxrPm2bo8eXcxGPN30gxhQSyMAAAAASUVORK5CYII=>

[image2]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABIAAAAYCAYAAAD3Va0xAAABF0lEQVR4Xu2TvUoDQRRGr1qZSkTSpBQMIjZ5AAvjA0jE0jYJ1jZ2go1VQLCxsgkk8SUELRQUkQTSBEJIo502kkLUnHFm2J27m8J+Dxx25n7D/C0jkvFftnGIr/iGzTD+4xFHOBA7thGkijZ+4A+uqmwBT/AWC2GUpItH+CvpK57hvi5qiniNS/iJ75gLRojcYV7VEtTw0LUvxe6qGsWyiM+x/kxauO7am2InMkf1lPEi1p/Ji+rfiJ1sy/VPcS+K0/H3E6cidiJfN/ezEsXp1CW6H4/53WP8wjV8CuN0Orihi3Asdlf3eK6yBHPYd1+NOcpE7GS7KktwgD2c14HjCr9xWQeeHbHvyqxoNE/DvDlNCR90MSMDpvMbNCf6RtASAAAAAElFTkSuQmCC>

[image3]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABUAAAAYCAYAAAAVibZIAAABUElEQVR4Xu2TvSuGURjGr3yG0ccgi0Rm/gFhY5LeySQfm11iIBlMwkKSQQaTycoiyiDMIhRWJUW4bvc5zrmf5ynv8m7Pr351zn2dj/ec57xATqnppjf0yXli4xQDCGMf6K6NLTv0jn7QikTmqaOH9Jse0DIbp7mgy9AJrYnMs0RnoGMmE1mKJnpMR6ET+mz8SxedpavQMR02TjNM52kvdMKYjVEOvbtqekUfbZyN7C4LyrFl0UUbY4r20Eb6hX8+juec1kA/0Cfdi7IWuuLaciLZdDzE2TRA79MjL+A06m/Setdegy7aHuJsZPeFqH9En117iI6ECNco8j7XaX/U34b+mmbo2/XIC5F6Ufd5SWuj/hx0sjzytqhecPWJqJbJIL2nVVFNjiuTp6OasOXqnYn6H/J/l7t5o+/0FXq3Pjujla6/QW8Rxr7QfZfl5JSKH+4dR2Zfvsd0AAAAAElFTkSuQmCC>

[image4]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABIAAAAYCAYAAAD3Va0xAAAA/klEQVR4Xu2Tv0qCcRSGXyRnhfAOrL3JURDBQRo0aGkUu4QuwxAnJydpbpAo8AaEhgYXK0yQamsIdOrP+/McPuRNpW//HnhweI7Hj6MCCXEp0Wf6Qqf+OqEV70P6RB/X7HjbyDX9oWUN5BTWejQt7Q/v9JPuaSAt2KITDcohbPBGg3NPv+m+BqUJW3ShgWTpF33QsIk+bNEZ7OkOaN5teGtH0zuY0wW9XfPODbcLi2rR9BbCp4fBgQbn3/c5x/b7ZBDjPlewRQUN5Bgx7rPr93MJW1TXoBzBBsNxlRQdw3pOWkQR9v/5gH1b4YlmtApbMKKv3pb0jXZX70xIMH4BCVZB68f13h4AAAAASUVORK5CYII=>

[image5]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABIAAAAXCAYAAAAGAx/kAAAA50lEQVR4Xu2SPwtBYRSHD0koA99AFrNsFoNiN5h9AZRJ+RJis5CUxWewUWaJFD6ABYWN3+m88d4Xue9+n3q695zf7fT+uUQetiTgHh605w4mVd5T9Va5gSGVfaUIH/AI41o/Cs9wAkswrGU/WZIMy2m9LmxptSuqJINGqm7Czjt2Twze4B024Bj6HV9YMCRZ1RwGjcyKCsmglRnYkIILkivmYVln/B8+mxlMa/WV5Lz43RUROIV5oz8gWVXN6H/ggxmSrbSNjCmQDFrDgJG9KJP8wbx8vmp+5rS8Di8kW+PsBPta7uHxBGdvMTS/7sIKAAAAAElFTkSuQmCC>

[image6]: <data:image/png;base64,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>

[image7]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAC8AAAAYCAYAAABqWKS5AAACy0lEQVR4Xu2WWahNURjHP0OGBxkyJVxkJg94IOKIFyEheUBCygMZEokkiRSiPIiEUPIiQ8iDkiTCkxdDrilTkiRDmf7/+63lfvdr7XNO9+zzovurX+fs/7eHtddee60t0sT/Qw8fVEAr2MWH1WIyPOvDCmgLb8EhvpBiHHwMX8O34fcpfAJr4Q24ULRHPP3gGzjQFypkhmgbuvpCFqfhHzjWZG3gipDvNXnkLtzjw5y4DE/5MAve6SfYwhfAc/gT9jbZJPgOtjdZngyC3+EwX/D0FO3dC74AmsMv8DfsY3KO8wNmuxpcg/t96Jkv2vh1viDaw6wdMxlv6AdcbTIPO4Rjt1PYbg0Lkn53sjgEH/jQc1i0gaNcXgPvwJewm8njk5puMstGeDX8chJYA8/BnfCe2a8U60WvEzsgCWcbDovNcAPcJPqy8B04CjvW71rHBNGTDnY5mQoPmu2b8IPoWsCO+Co6EZTDbNHrZI772IvskZlB9mhBsi+yCP6S9BDgjdtF5j08Ef7Pg1NMjbADprksMkK0bRN9IbJAdIetLi/GWvjZhwmGip57sS8YlsNlPgz0Ej3e3/A/jojuUHB5MdgYHtPZFxwrRffjYtYY4vAc4AuRWvhNdDYoF84iPOkYXxAdUlvC//PwlanViL60Efb6NtjMZJYlkj086xYCNuK6L5Sgv+hxHHIWXoQLyxnRpZ2dcjvUOL0el/qFjuN8JHwo6Ref7IAvfMhPgEfwo+gFOANwOpxjdyrBM0m/J5xpOD+fFH3R+I3E7YvS8Pyj4XDR1Zs3loKdcMmHecDVlQ1K0V3065CwYX1NzbIbbvdhgMdxCp/lC3nA8cshMt4XyoSN4wLGL1IuZh7OQHwqqW+tXGDPcX3IeuzF4Et6H64SHfuWDqLrw1yX50pLeAUu9YUy4fGpWW6fNJyVqkY7yfdC/MTeJY17mk1Ujb9aEIYaF/HNZgAAAABJRU5ErkJggg==>

[image8]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAcAAAAXCAYAAADHhFVIAAAAhklEQVR4XmNgGHjAAcTxQCyOLgECaUD8H4gj0CVAQAqIw4CYGV0CL9AFYh10QRBoB+LJQPyMAWIvHBgC8QYo+xYQr0SSAzvdAIpBLgXxUQArEL8C4gVo4mDgzwDR5QjESkDciCzZA8QvgZgRiCcCsSqypCUQvwHiSQxoroUBHiDmRRccOgAA22YSErYguUoAAAAASUVORK5CYII=>

[image9]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAADMAAAAYCAYAAABXysXfAAADNUlEQVR4Xu2XWahNYRiGP/OQeR7bUUhEyJDEBeUChXBSyBgXQpJM5VyRKFzIXIZcGDKGzC4UImXIhfEQJYpC5ul9ff9yvv3ttfbeF3srOU+97c77/uvfa6/1f///HZEK/h8aQrW8WSCqQ029mS81oIHQMKily+JoBV2F6vugQPAhXYY6+yAbnaCj0F2oFFoJvYVOQc3Lh6XBH34FmmK8rdAT6D70EHoMjQ/ZXqjMZPzsF7Ie0NOQ8xreR8QI6BHUzHiJLIZeQJOgqsZPQddEJ69j/Ig1onkl5w+FfkLHJX0+Mjtku6HaLmsDvYbGii4vy0loj/MyWAt9hvr4IDBZ9MuXO7+e6Jvr73zSVfSaEz4QvVFmm30AxkGLvBngyvkEdfFBxEjRiTf4wMCa4Jjrzp8L3XFeRHQNl6CFb5Aes/0u45I9Gz6TOA+t9yZhwT6HPop+eRJ1oR/QG+efgQ47L6Km6A3fc/4E0Zpids5lC6HRzvNskYQHOF90Ug7IBpcRx7FgLQ9EayaJD9Ar8zfrgz+AdcH5bpiMhX3E/J0EfzCvbeQDvlIG033gmCk67pjxqkBfoFnG8zyDvkn55sCaGw5VE52vLPhko+hulgu+OV6bVjeckEXPoK8NYuCP4Lh5xksFb7DxPLdEx3A5t4YOmey96OZBukvu1RHRTXTOQdZkkbEOqLgtN4JPgGP4lFkHET1FJ+VnEhdFx7SHtovucBE8T5jxobLuWpgsG21FrxviA9YAg2zFv1N0zETntwv+GOdbDoqO4VL0u+XNkHEJL3FZNtiZ8LoOPtgVAu7tcfBU55qf4wPRHY7XJp0JhG+DY9gN+N7qQshuS/obz8VU6LtkHqi/+y5ut+x7GrtsGvQVmuF8C5feNm8aVove8AIfgAOiWYkPcrBCdInG0lu0ULnN8g2sEj0bTkMDzLg4dojWRRJskThvxlMEm6BL3syDfRLfVfyBvVMv0QONnXIqPU6E9cJzJGmZdJTkTpcbR77fE1FZtDEd5YNCwMnZ2S71QZHgkmf98YwrCmzN34meI8WkAfRStEktKstE2/lisk70/6u/QinUxJsFgh0ENyYu6wr+GX4BmuKvRHcrV2EAAAAASUVORK5CYII=>

[image10]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABUAAAAYCAYAAAAVibZIAAABVElEQVR4Xu2TvytFYRjHv34kl4lFyoBMJkVKMiqjBVH+gIvJoChlEZsMFkU3ZWIy0E02m0Fd3VJ+ZpZBYvLj+3ieo/c+znUsJvdTnzrv99vzntM57wFK/BVn9JZe0Gt6Q1toLc3bOupyNPU5BaTpHXRW3LT8iyX6Tmdpmet2rJugFa4bope0E9/nMAUdnPEFWYN2o76Adj0+jBiDDi67vIE+WjfpunaacVkBA9DBdZfLesO6edft0iaXFdANHZT3F9EB3XDYupWgk4dYCNaxtEEHj4JsjzbSfusyllfSLK2xdVHqoYOnth6kc3bdZZ3cRJCPOm7XP1JO36Dnrooe0mrrWqGbHtM6eoCY41OMB/pEp+lIkMtGsqn8CKu0N+gSuYIO77tcnuqVPtNt1yVyAn0FchI89/SFNrs8EfmiWz40zumiD39DH/QUxCHnMvEIlfjvfADWbEg92dj82wAAAABJRU5ErkJggg==>

[image11]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAFMAAAAYCAYAAACGLcGvAAAEUUlEQVR4Xu2YaahVVRTHV2WD0GQWNoCvgQYbvjQoRRliSFDRQGWFpU000vRFpcEHRqEIahJBFDQXRRRlQuUHKZpooCKatAksBLWyqGgw+/9c+3D3Wfecc596H9Tt/eDPvWetffY5Z5+91l77mA0xxP+FEdLwaOwh9omGgbK9NF46Rdor+KrYW3pL2iU6eogHpfOisYmDpeekj6V+6U7pJ+lFaVSrWQkG/k3p4ugQO0ufSd9Jq8z73aHUwuwu6RtpufSF9HrZvVlcat7niqSvpQXJd2M6LnxfSdcm3zbSJ+bnYuf3iORjUq2Wjk/Hjcw0f+CLpGGZvU9627zzHTN7wTxz/1bRkXGl9KO0waoHfaL5tY+25n42BVLOemmttFPwHW5+L59KewQf570jzTafDDnTzcdh22AvMV/6XRobHYlp5hefFexcjJl7XLBHnpCmmvfxXvDBydLd0dgF1ki/RqP5AHIvRE1ktPRMNCa2M48gJkclZ5h33PQw5ETa8MZyrpc+CrYqPjCfcYQw/cTBv106J9i6wefm14upZW6yE7YRcuOYaMy4zfx52mDB+Fb6zXzA6iBM/pZ+CPaXpWeDLUIefjL9P9/8IR5ruTfyirWHWzcgl3O9/NkOkJ42z+N/WTmtjJMWZsdVXGA+Fm33e5P5xe6NjgAziXYsEDkkcHJmE1dIV6X/5Boe4g9pz2QjR72b/nebJeb3fVhme1w60HwxxJfnxRek3bLjKkiFnHdmdCxNjsuiI8CA0O75zMbKx6DU5o8EN5+HzSzzvggXYPFZ1HJ3lUfMr3VCOj7RWi//teTbNx0TNdel/00w2Jx3TW5klrDo4GB6N8Eg0u6GzNaXbAxGE++HY2YkL4H0wj2QL88qtaiHMoXKYXJ01EDZxT2eLm1tXuIVtfDi5DvSPDpIWY2rdMb3FhZj6kNiH1WVPAWECG1WWjmRcxPFzdRxiLXyZc6j5ucyKK9KI8vuWooIeSg6aug3b3+JeUmWRxF94DtJulU6NfN1gnTHJGgz0mHT4sPqRpsLg32/ZD872HPIlaVwSBxrfi7VQawQmmDm8NCd8loBYct1+qWXzFNTAQsNPtaNulKoiiK9XR4dxds5NzoSvE1WvKpcwgrPuTOiI+Mpa+0gIgwi5xc7k8GACcA12DBMCj7CFB8VCkX8QNnfWjO6BFskOnvD2kONLdmfVvEGMgj9+6IxwSDSd902dKr5TVHnDhZ8W+Aa5McIW0h890RHB3gpnMegtnGM9KF5mcMMnGNe7BIWnfahD0jLgm1X6Uvz2hX9It1SauGQs9n78rVpsCCdEJLk7ggfLXjZbfViB66W1ll5y10Cx1HSFPO32Vd210K+ZBcRdxj/FqghJ0RjgqhksDeV+61zYb9ZUG5Q/N4cHT3Koebl5EHR0S1Ok362Lfh4+h+C1Nf0DaMrkBMfjsYeg4WSbwgDLey3iH5p92jsIe6w3n6+IXqOfwB9EecdQiqmWAAAAABJRU5ErkJggg==>

[image12]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACgAAAAYCAYAAACIhL/AAAACfElEQVR4Xu2WSUhWURiGv2YjGzWjIqJFhAQtDA1EalHgIoKKalXRiG60SBDDQFeFEFSLNuHCEjetoqBooKJNRa0iQhopCqIWgdE8vi/fvXb+l+vhCn/RwgcehPc7nv+ce89wzUb4/5gOJ2oYYTycqWFeJsDlcDWcLbUs5sDbcKoWInAyN2GlFmIsgmfhA9gJD8EBeBHO+tOsAE7mFtyuhYRJsEXDhDXwKazQQhb74Wu4FY4N8vnwDnwGS4M85bB5fVSQ8XW3wSvwM/wQ1JQLsE9D5Qj8Amu0kLAN/oIdkk8xf8K1kvOJNMEV8IbFB8i3xkks1kLKWvMfP66FAK4xtrkr+R54XzKFyyM2QHIVHtOQcFG/gp/MBzEUk+FP+E7yy/CMZEqeAZ6wISa6z/zJsEEMvkK2eyL5Y/M1GCPPAFvN+5+hBS5iFnZpQWgwb3cuyMbAr7AxyLLIM8D15v0XrMNx5huDhWVhIQMOjO32Bhl3N7OVQZYFB/hRQ2GJeV/cVIPw/OK6olnHRwpnxTYvYUmQV5l3yr8xOECu8RjzzPtapQWuKRZiG+SkeZstki9I8g2SK3kGyFuLfS3UwqmksEkLCbwdvsNmLZjvbP4vD+QYHCDPuRg74A/z+7kA3rM8OngnlkltJ/wGd0sewtferaFwzXwzxZbRQfhCw5RqeM/8yOCT6oIP4SVYF7TLogdel4xwso/Mr0feNJQPoh/WB+1STsPzGobw7l0KN5t/wXCH5oHr760Vbp7hMtp8Muu0UAzYOb982rUwDLiEnpufq38FfjK9h3O1kINp8A3cqIVicwD2apiDo+bfnP+ETliuYQR+qHBDcpmMUHR+A6cJgjGfrXLgAAAAAElFTkSuQmCC>

[image13]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABcAAAAYCAYAAAARfGZ1AAABcElEQVR4Xu2UyytFURTGv0j5F7xSJh555FEiKSVMSMmIMSPF2ECZKErmHgPCTSkUpZT4A0wNhAzECKUkFN9q7evsu5zb6bgGBverX539fat12uvss4F/rjoyRbpInsky0gAZI1XkkGykxpnpiCy45zbySWqDOFWt5JLckXty69Y35JyskObvaqCdVLvnGmjzsiAOVwJa2OLWuaSUbJIP0ut8X/Nk15phuiJP0Ka+KqAvPTW+7OCA5Bv/h4qhDfZsQHVAswvPq4SOSxqXONJqCNpgwgbUEjQbdOtCskWaSCOZJeUuC9UitEG950mTdfJIRjz/GFqb5B0RZ122LEUyQ+GMvELPcIFXF1vJee94nnzUNfKAiC1HaRjh8+5x/qTxY2kZ2qTB+OPOnzZ+LF1Dz3eO8behzeUlv5LMUxrs24A6gWajbr1KioI4veSukBMiH+yFPEPvkm6vpo+8QW++GTLnZX8i+fP6SacNssoqc30BFt1OCh2zdSoAAAAASUVORK5CYII=>