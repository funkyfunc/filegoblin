# **Engineering High-Performance Recursive Web-to-Markdown Crawlers in the Rust Ecosystem**

The development of a recursive web-to-markdown crawler represents a significant escalation in architectural complexity compared to simple single-page scrapers. Transitioning from fetching a static URL to autonomously navigating the vast, interconnected graph of the World Wide Web requires a sophisticated synthesis of network orchestration, robust state management, and ethical compliance. The Rust programming language has emerged as a premier choice for these systems due to its unique convergence of high-level abstractions and low-level control, facilitating the creation of crawlers that are simultaneously memory-safe and exceptionally performant.1 By leveraging the ownership model and an advanced asynchronous ecosystem, engineers can build tools capable of processing thousands of pages per second while maintaining a negligible resource footprint compared to equivalent implementations in managed languages like Python or JavaScript.2

## **Architectural Foundations of Recursive Crawling**

A recursive crawler is essentially a directed graph traversal engine that operates over a network. Every web page is a node, and every hyperlink is a directed edge leading to another node. The primary objective is to visit every reachable node within a defined scope and transform its unstructured HTML content into structured Markdown. The architectural design must account for the non-deterministic nature of the web, where links can be broken, servers can be slow, and directory structures can be infinitely deep.

In Rust, the architecture typically centers around an asynchronous runtime, most commonly Tokio, which handles the orchestration of non-blocking I/O tasks.1 This allows the crawler to manage a high degree of concurrency without the overhead of native OS threads. The system's core components include a frontier management system, a fetcher-worker pool, a politeness controller, and an extraction pipeline.

### **The Role of Memory Safety and Ownership**

One of the primary challenges in recursive crawling is managing shared state across multiple concurrent workers. In languages without strict memory guarantees, this often leads to data races, deadlocks, or "use-after-free" errors as different parts of the system attempt to update the list of visited URLs. Rust’s ownership model and the Send and Sync traits ensure that these issues are caught at compile time.1 For instance, when sharing a thread-safe HTTP client or a visited URL set, the use of Arc (Atomic Reference Counting) allows multiple tasks to own a reference to the same resource, while structures like DashMap provide fine-grained locking to prevent contention.1

## **State Management and URL Deduplication**

State management in a recursive crawler focuses on two primary goals: tracking the "frontier" (the queue of URLs yet to be visited) and maintaining a "history" (the set of URLs already processed). Failure to manage these correctly results in redundant work and infinite loops, especially in the presence of circular links where Page A links to Page B, which links back to Page A.1

### **Evolution of Deduplication Structures**

For a beginner-level tool, a std::collections::HashSet\<String\> wrapped in a Mutex is often sufficient. However, as the crawler scales, this approach encounters two bottlenecks: lock contention and memory exhaustion. In a high-concurrency environment, workers spend more time waiting for the lock on the HashSet than they do fetching pages.

A more advanced solution involves sharded data structures. The DashSet (provided by the dashmap crate) is internally partitioned into multiple segments, each with its own lock. This allows a worker checking a URL in segment A to proceed simultaneously with a worker updating a URL in segment B.1

As the crawl size reaches millions of URLs, even a sharded set becomes problematic due to the memory cost of storing raw strings. A typical URL can be 100 characters or more, meaning 1 million URLs could consume 100MB of RAM just for the strings, excluding the overhead of the hash map itself. To mitigate this, developers often transition to probabilistic data structures or hash-based deduplication.

| Deduplication Strategy | Space Complexity | Accuracy | Contention Profile |
| :---- | :---- | :---- | :---- |
| HashSet\<String\> | ![][image1] | 100% | High (unless sharded) |
| DashSet\<String\> | ![][image1] | 100% | Low |
| HashSet\<u64\> (Hashed) | ![][image2] | ![][image3] | High |
| Bloom Filter | ![][image4] | Probabilistic | Zero (if lock-free) |
| Persistent Key-Value (RocksDB) | ![][image5] | 100% | I/O Bound |

### **Probabilistic Membership with Bloom Filters**

For massive-scale crawls where absolute 100% accuracy is less important than memory efficiency, the Bloom filter is an essential tool. It uses a bit array of size ![][image6] and ![][image7] different hash functions. When a URL is visited, it is hashed ![][image7] times, and the corresponding bits in the array are set to 1\. To check if a URL has been visited, the same ![][image7] positions are checked; if any bit is 0, the URL is definitely new. If all bits are 1, the URL is likely already visited.5

The false-positive rate ![][image8] of a Bloom filter can be calculated as:

![][image9]  
Where ![][image10] is the number of elements inserted. This mathematical guarantee allows engineers to trade a small percentage of skipped unique pages for a massive reduction in RAM. Modern Rust implementations like fastbloom utilize L1 cache-friendly blocks and SIMD instructions to make these checks even faster than standard hash-based sets.7

## **Graph Traversal: BFS versus DFS in Web Contexts**

The strategy used to "walk" the website determines the discovery order and the memory profile of the frontier.

### **Breadth-First Search (BFS)**

BFS explores the web graph layer by layer. It processes the seed URL, then all URLs found on the seed page, then all URLs found on those pages, and so on. BFS is generally the industry standard for web crawling for several reasons.9 Firstly, BFS prioritizes pages closer to the home page, which are typically the most relevant. Secondly, it naturally aids in politeness; because it spreads its requests across many different branches of a site, it is less likely to hammer a single sub-directory or server-side resource in a short burst.10

The primary drawback of BFS is memory usage. The "frontier" (the queue of discovered links) grows horizontally. For a site with a high branching factor ![][image11], the number of URLs in the queue at depth ![][image12] is approximately ![][image13]. In Rust, managing this frontier requires a robust VecDeque or a persistent queue if the scale exceeds available RAM.1

### **Depth-First Search (DFS)**

DFS dives deep into a single path before backtracking. While DFS is more memory-efficient in terms of the number of nodes stored in the search stack (![][image14] vs ![][image15]), it is frequently problematic for web crawling.9 A DFS crawler can easily get trapped in "infinite" directory structures—such as a calendar that dynamically generates links for the next month indefinitely—without ever exploring other sections of the site.10

Some specialized crates, such as crawly, utilize DFS for specific use cases, but for a general-purpose recursive Markdown crawler, a BFS approach, often with a depth limit to prevent wandering into traps, is highly recommended.13

## **Concurrency and Asynchronous Execution Patterns**

Efficiency in Rust is achieved by maximizing the utilization of the CPU and network interfaces. Because web crawling is heavily I/O bound (waiting for server responses), an asynchronous runtime is far superior to a multi-threaded blocking approach.1

### **The Worker Pool Pattern**

The most idiomatic way to manage concurrency in a Rust crawler is a worker pool driven by channels. A central manager maintains the frontier and distributes URLs to worker tasks.1

Rust

use tokio::sync::mpsc;  
use std::sync::Arc;

async fn worker(mut rx: mpsc::Receiver\<Url\>, tx: mpsc::Sender\<Url\>, state: Arc\<CrawlerState\>) {  
    while let Some(url) \= rx.recv().await {  
        // Fetch, Parse, and Extract  
        let new\_links \= process\_page(url, \&state).await;  
        for link in new\_links {  
            tx.send(link).await.unwrap();  
        }  
    }  
}

This pattern uses tokio::sync::mpsc (multi-producer, single-consumer) or async\_channel to coordinate work. A critical refinement in production systems is the use of a Semaphore to limit the number of active requests. This prevents the crawler from opening thousands of simultaneous TCP connections, which could trigger local OS limits or be interpreted as a DDoS attack by the target server.1

### **Stream-Based Concurrency**

For processing a fixed or semi-static set of URLs, the futures::stream module offers powerful abstractions. The for\_each\_concurrent method allows the developer to define a concurrency limit directly on the stream.15

Rust

use futures::stream::{self, StreamExt};

let urls \= stream::iter(vec\_of\_urls);  
urls.for\_each\_concurrent(10, |url| async move {  
    fetch\_and\_convert(url).await;  
}).await;

While simpler to implement, this approach is often less flexible for a recursive crawler where the list of URLs is growing dynamically. The channel-based worker pool remains the robust choice for systems where the total crawl volume is unknown at the start.17

## **Link Filtering and Domain Scoping**

A recursive crawler must have strict "rules of engagement" to prevent it from accidentally attempting to index the entire internet.

### **Relative vs. Absolute URL Resolution**

HTML anchors often use relative paths like ../images/photo.jpg. To process these, the crawler must maintain the "base URL" of the page being currently processed. The url crate’s join method is the standard for this, as it correctly handles root-relative, path-relative, and protocol-relative links.1

| Link Type | Example | Resolution Logic |
| :---- | :---- | :---- |
| Absolute | https://github.com | No change needed. |
| Root-Relative | /about | Join with host: https://example.com/about. |
| Path-Relative | contact.html | Join with current path: https://example.com/blog/contact.html. |
| Protocol-Relative | //cdn.com/lib.js | Adopt current scheme: https://cdn.com/lib.js. |

### **On-Site Scoping Best Practices**

To ensure the crawler stays within the target website, every discovered link must be validated before being added to the frontier. A common mistake is a simple string prefix check, which can be fooled by subdomains or similar-looking domains. The correct approach is to parse the URL and compare the host() or domain() property with the seed domain.20

Advanced scoping should include:

1. **Domain Matching**: Reject any URL where the host does not match the target.  
2. **Scheme Filtering**: Only process http and https. Skip mailto:, tel:, ftp:, etc.  
3. **Extension Blacklisting**: Skip obviously non-HTML files like .pdf, .zip, .mp4, unless specifically required.1

## **Politeness and Ethical Crawling**

Ethical crawling is essential for maintaining the health of the web ecosystem and preventing the crawler's IP from being blacklisted.

### **Robots.txt Parsing**

The robots.txt file is the webmaster's primary tool for defining crawl boundaries. A compliant crawler must check this file before fetching any other resource on a domain. The robotxt and texting\_robots crates are the industry standards for this in Rust.22 These libraries parse the rules and allow the crawler to check if a specific path is allowed() for its User-Agent.

It is a best practice to cache the parsed robots.txt rules per domain for a period (often 24 hours) to avoid redundant requests. Furthermore, if a server returns a 429 "Too Many Requests" status when you attempt to fetch robots.txt, you should immediately stop and wait for a significant duration.23

### **Rate Limiting with Governor**

The governor crate provides a high-performance implementation of the Generic Cell Rate Algorithm (GCRA), which is more precise than traditional token buckets. Implementing rate limiting ensures that the crawler does not overwhelm the target server's CPU or bandwidth.26

A sophisticated crawler uses **keyed rate limiting**. Instead of one global limit, it maintains a separate rate limiter for every domain it visits. This allows it to crawl 10 different sites at full speed while remaining polite to each individual server.1

| Politeness Feature | Implementation Method | Purpose |
| :---- | :---- | :---- |
| User-Agent | Identify with URL/Email | Provide contact info to webmasters.4 |
| Robots.txt | robotxt crate | Respect exclusion zones.24 |
| Rate Limiting | governor crate | Prevent server overload.26 |
| Retries | Exponential Backoff | Handle transient failures gracefully.30 |

## **Content Transformation: HTML to Markdown**

The transformation pipeline is where the raw HTML data is distilled into its most useful form.

### **Content Extraction and Noise Reduction**

Directly converting an entire HTML page to Markdown often results in a file filled with navigation menus, sidebars, footer links, and advertisement placeholders. To produce "clean" Markdown, the crawler should first pass the HTML through a "Readability" algorithm. This algorithm, popularized by Mozilla's Firefox Reader Mode, uses heuristics (like text density and class name patterns) to identify the "main" article content and strip away the boilerplate.31

Crates like readability-rs or readable-rs provide native Rust implementations of this logic. This step is critical for producing Markdown that is actually readable and useful for downstream processing like LLM training or archival.31

### **Markdown Conversion Crates**

Once the HTML is cleaned, it must be mapped to Markdown syntax.

* **html-to-markdown-rs**: Highly recommended for production use. It is CommonMark compliant and handles complex structures like tables and nested lists with high fidelity.35  
* **html2md**: A lighter, more basic alternative suitable for simple pages.  
* **htmd**: Offers a highly extensible API, allowing the developer to define custom rules for specific HTML tags.35

### **Normalization and Canonicalization**

Websites often serve the same content on multiple URLs (e.g., example.com/page, example.com/page/, and example.com/page?utm\_source=twitter). To avoid processing the same content multiple times, the crawler should normalize URLs before deduplication. This involves:

1. **Lowercasing the host**.  
2. **Removing trailing slashes**.  
3. **Stripping tracking parameters** (like utm\_ or ref).  
4. **Removing fragments** (anything after the \#). The urlnorm crate provides pre-built heuristics for these common normalization tasks.37

## **Resilience and Robust Error Handling**

In the wild, a crawler will encounter a multitude of failure states. A "robust" crawler must handle these without crashing or losing progress.

### **Handling HTTP Status Codes**

The crawler must differentiate between transient and permanent errors.

* **404 Not Found**: A permanent error. Log it and move on.1  
* **500/503/504 Server Errors**: Often transient. These should be retried using exponential backoff.30  
* **429 Too Many Requests**: A signal to back off immediately. The crawler should respect the Retry-After header if present.23

### **Exponential Backoff and Jitter**

When a request fails due to a transient error, retrying immediately can lead to a "thundering herd" problem that keeps the server down. The backoff or tokio-retry crates allow for implementing exponential backoff with jitter.30 Jitter adds a random element to the wait time, ensuring that hundreds of concurrent workers don't all retry at the exact same millisecond.40

Rust

use tokio\_retry::strategy::{ExponentialBackoff, jitter};

let retry\_strategy \= ExponentialBackoff::from\_millis(100)  
   .map(jitter)   
   .take(3); // Max 3 retries

## **Recommended Library Stack for 2026**

For a developer building a high-performance recursive crawler, the following crate selections represent the current "gold standard" in the Rust ecosystem.

| Component | Library Recommendation | Reasoning |
| :---- | :---- | :---- |
| **Runtime** | tokio (with full features) | The most mature and widely supported async runtime.1 |
| **HTTP Client** | reqwest | Supports async, connection pooling, and cookie management.43 |
| **HTML Parsing** | scraper or tl | scraper is standard; tl is significantly faster for large docs.4 |
| **Visited Set** | dashmap | Enables concurrent state updates without global mutex locks.1 |
| **Rate Limiting** | governor | Efficient GCRA implementation for complex politeness rules.26 |
| **Robots.txt** | robotxt | High-fidelity parsing of exclusion rules and sitemaps.22 |
| **URL Handling** | url & urlnorm | Essential for normalization and safe path joining.19 |
| **Markdown** | html-to-markdown-rs | Best-in-class performance and CommonMark compliance.36 |
| **Readability** | readabilityrs | Necessary for stripping navigation noise from output.31 |

For those seeking a more framework-oriented approach, the spider crate is the most advanced crawler-specific library. It integrates many of the above features—such as lock-free deduplication, automated sitemap discovery, and even optional headless browser rendering via the Chrome DevTools Protocol—into a single, high-performance package.14

## **Conclusion: Strategic Implementation Path**

Building a recursive crawler in Rust is an iterative process. A developer should begin by perfecting the single-page conversion logic, ensuring the Markdown output is clean and semantic. From there, the introduction of an asynchronous worker pool and a robust visited-URL tracking system (using DashSet or a Bloom filter) enables the tool to "walk" the site structure.

The final, and most critical, phase involves implementing politeness and resilience features. Respecting robots.txt, enforcing per-domain rate limits with governor, and handling server errors with exponential backoff transforms a basic script into a professional-grade data extraction tool. In the modern web environment, where anti-bot measures are increasingly common, the performance and control offered by Rust’s systems-level architecture provide the best foundation for a robust, ethical, and long-lived web crawler.

#### **Works cited**

1. How to Build a High-Performance Web Crawler with Async Rust \- OneUptime, accessed February 22, 2026, [https://oneuptime.com/blog/post/2026-01-25-high-performance-web-crawler-async-rust/view](https://oneuptime.com/blog/post/2026-01-25-high-performance-web-crawler-async-rust/view)  
2. How to Build an Efficient Rust Web Crawler \- Thunderbit, accessed February 22, 2026, [https://thunderbit.com/blog/efficient-rust-web-crawler](https://thunderbit.com/blog/efficient-rust-web-crawler)  
3. Web Scraping with Rust: A Performance-Focused Implementation Guide \- Rebrowser, accessed February 22, 2026, [https://rebrowser.net/blog/web-scraping-with-rust-a-performance-focused-implementation-guide](https://rebrowser.net/blog/web-scraping-with-rust-a-performance-focused-implementation-guide)  
4. Rust web scraping: Complete beginner guide \- ScrapingBee, accessed February 22, 2026, [https://www.scrapingbee.com/blog/web-scraping-rust/](https://www.scrapingbee.com/blog/web-scraping-rust/)  
5. Advanced Algorithms Every Senior Developer Must Know: Part 3 — Bloom Filters \- Medium, accessed February 22, 2026, [https://medium.com/@mr.sourav.raj/advanced-algorithms-every-senior-developer-must-know-part-3-bloom-filters-2e6092e343aa](https://medium.com/@mr.sourav.raj/advanced-algorithms-every-senior-developer-must-know-part-3-bloom-filters-2e6092e343aa)  
6. Bloom Filters: Fast, Cheap, and Slightly Wrong (On Purpose) \- Medium, accessed February 22, 2026, [https://medium.com/@ritheshvr/bloom-filters-fast-cheap-and-slightly-wrong-on-purpose-9db252fa7d56](https://medium.com/@ritheshvr/bloom-filters-fast-cheap-and-slightly-wrong-on-purpose-9db252fa7d56)  
7. fastbloom: The fastest Bloom filter in Rust \- Reddit, accessed February 22, 2026, [https://www.reddit.com/r/rust/comments/1bmozok/fastbloom\_the\_fastest\_bloom\_filter\_in\_rust/](https://www.reddit.com/r/rust/comments/1bmozok/fastbloom_the_fastest_bloom_filter_in_rust/)  
8. Sbbf-rs Fastest (without asterisks) bloom filter library in Rust \- announcements, accessed February 22, 2026, [https://users.rust-lang.org/t/sbbf-rs-fastest-without-asterisks-bloom-filter-library-in-rust/96478](https://users.rust-lang.org/t/sbbf-rs-fastest-without-asterisks-bloom-filter-library-in-rust/96478)  
9. Breadth-First Search vs Depth-First Search: Key Differences \- Codecademy, accessed February 22, 2026, [https://www.codecademy.com/article/bfs-vs-dfs](https://www.codecademy.com/article/bfs-vs-dfs)  
10. Understanding DFS vs BFS in Web Crawling: A Practical Perspective | by Se Hyeon Kim, accessed February 22, 2026, [https://medium.com/@seilylook95/understanding-dfs-vs-bfs-in-web-crawling-a-practical-perspective-8129c93bfb02](https://medium.com/@seilylook95/understanding-dfs-vs-bfs-in-web-crawling-a-practical-perspective-8129c93bfb02)  
11. Why does a breadth first search use more memory than depth first? \- Stack Overflow, accessed February 22, 2026, [https://stackoverflow.com/questions/23477856/why-does-a-breadth-first-search-use-more-memory-than-depth-first](https://stackoverflow.com/questions/23477856/why-does-a-breadth-first-search-use-more-memory-than-depth-first)  
12. DFS vs BFS — \[Notes\] \- Tarun Jain \- Medium, accessed February 22, 2026, [https://tarunjain07.medium.com/dfs-vs-bfs-notes-f570cb427e17](https://tarunjain07.medium.com/dfs-vs-bfs-notes-f570cb427e17)  
13. aichat-bot/crawly: A lightweight async Web crawler in Rust, optimized for concurrent scraping while respecting \`robots.txt\` rules. \- GitHub, accessed February 22, 2026, [https://github.com/aichat-bot/crawly](https://github.com/aichat-bot/crawly)  
14. spider-rs/spider: Web crawler and scraper for Rust \- GitHub, accessed February 22, 2026, [https://github.com/spider-rs/spider](https://github.com/spider-rs/spider)  
15. How to Use Rust Futures and Streams \- OneUptime, accessed February 22, 2026, [https://oneuptime.com/blog/post/2026-02-03-rust-futures-streams/view](https://oneuptime.com/blog/post/2026-02-03-rust-futures-streams/view)  
16. Reqwest: Multiple concurrent requests by priority \- Rust Users Forum, accessed February 22, 2026, [https://users.rust-lang.org/t/reqwest-multiple-concurrent-requests-by-priority/128415](https://users.rust-lang.org/t/reqwest-multiple-concurrent-requests-by-priority/128415)  
17. Difference futures::streamExt Buffered vs for\_each\_concurrent \- Rust Users Forum, accessed February 22, 2026, [https://users.rust-lang.org/t/difference-futures-streamext-buffered-vs-for-each-concurrent/69650/6](https://users.rust-lang.org/t/difference-futures-streamext-buffered-vs-for-each-concurrent/69650/6)  
18. Difference futures::streamExt Buffered vs for\_each\_concurrent \- Rust Users Forum, accessed February 22, 2026, [https://users.rust-lang.org/t/difference-futures-streamext-buffered-vs-for-each-concurrent/69650](https://users.rust-lang.org/t/difference-futures-streamext-buffered-vs-for-each-concurrent/69650)  
19. Normalizing URLs in Rust: Converting Relative to Absolute URLs \- Multi-Threaded Web Crawler \- ProjectAI, accessed February 22, 2026, [https://projectai.in/projects/1289d7e5-3080-462c-938a-d02832674f6a/tasks/f19797b7-95a6-4e43-8de7-4eb088c21413](https://projectai.in/projects/1289d7e5-3080-462c-938a-d02832674f6a/tasks/f19797b7-95a6-4e43-8de7-4eb088c21413)  
20. Web crawler in Rust, accessed February 22, 2026, [https://rolisz.ro/2020/web-crawler-in-rust/](https://rolisz.ro/2020/web-crawler-in-rust/)  
21. referenceFilter : How to crawl domain and sub domains for 1000 websites · Issue \#563 · Norconex/crawlers \- GitHub, accessed February 22, 2026, [https://github.com/Norconex/collector-http/issues/563](https://github.com/Norconex/collector-http/issues/563)  
22. robotxt \- Rust \- Docs.rs, accessed February 22, 2026, [https://docs.rs/robotxt](https://docs.rs/robotxt)  
23. Smerity/texting\_robots: Texting Robots: A Rust native ... \- GitHub, accessed February 22, 2026, [https://github.com/Smerity/texting\_robots](https://github.com/Smerity/texting_robots)  
24. robotxt \- crates.io: Rust Package Registry, accessed February 22, 2026, [https://crates.io/crates/robotxt](https://crates.io/crates/robotxt)  
25. Texting Robots: Taming robots.txt with Rust and 34 million tests \- Smerity.com, accessed February 22, 2026, [https://state.smerity.com/direct/smerity/state/01FZ5M005VE4B5V2F424GHWMX6](https://state.smerity.com/direct/smerity/state/01FZ5M005VE4B5V2F424GHWMX6)  
26. governor \- Rust \- Docs.rs, accessed February 22, 2026, [https://docs.rs/governor](https://docs.rs/governor)  
27. governor \- Rust \- Parity, accessed February 22, 2026, [https://paritytech.github.io/try-runtime-cli/governor/index.html](https://paritytech.github.io/try-runtime-cli/governor/index.html)  
28. Building a Rate Limiter in Rust \- Medium, accessed February 22, 2026, [https://medium.com/@dmytro.misik/building-a-rate-limiter-in-rust-48ce37d2ae07](https://medium.com/@dmytro.misik/building-a-rate-limiter-in-rust-48ce37d2ae07)  
29. Writing a Web Scraper in Rust using Reqwest \- Shuttle.dev, accessed February 22, 2026, [https://www.shuttle.dev/blog/2023/09/13/web-scraping-rust-reqwest](https://www.shuttle.dev/blog/2023/09/13/web-scraping-rust-reqwest)  
30. How to Implement Retry Logic with Exponential Backoff in Rust \- OneUptime, accessed February 22, 2026, [https://oneuptime.com/blog/post/2026-01-07-rust-retry-exponential-backoff/view](https://oneuptime.com/blog/post/2026-01-07-rust-retry-exponential-backoff/view)  
31. theiskaa/readabilityrs: A Rust port of Mozilla's standalone readability library \- GitHub, accessed February 22, 2026, [https://github.com/theiskaa/readabilityrs](https://github.com/theiskaa/readabilityrs)  
32. readability-rust \- crates.io: Rust Package Registry, accessed February 22, 2026, [https://crates.io/crates/readability-rust](https://crates.io/crates/readability-rust)  
33. readable-rs — Rust text processing library // Lib.rs, accessed February 22, 2026, [https://lib.rs/crates/readable-rs](https://lib.rs/crates/readable-rs)  
34. Readability \- Rust \- Docs.rs, accessed February 22, 2026, [https://docs.rs/readability-rust](https://docs.rs/readability-rust)  
35. Announcing html-to-markdown v2: Rust rewrite, full CommonMark 1.2 compliance, and hOCR support : r/Python \- Reddit, accessed February 22, 2026, [https://www.reddit.com/r/Python/comments/1o3sqqz/announcing\_htmltomarkdown\_v2\_rust\_rewrite\_full/](https://www.reddit.com/r/Python/comments/1o3sqqz/announcing_htmltomarkdown_v2_rust_rewrite_full/)  
36. html-to-markdown-rs — Rust web dev library // Lib.rs, accessed February 22, 2026, [https://lib.rs/crates/html-to-markdown-rs](https://lib.rs/crates/html-to-markdown-rs)  
37. urlnorm \- Rust \- Docs.rs, accessed February 22, 2026, [https://docs.rs/urlnorm](https://docs.rs/urlnorm)  
38. normalize-url-rs \- crates.io: Rust Package Registry, accessed February 22, 2026, [https://crates.io/crates/normalize-url-rs](https://crates.io/crates/normalize-url-rs)  
39. progscrape/urlnorm: URL normalization library for Rust \- GitHub, accessed February 22, 2026, [https://github.com/progscrape/urlnorm](https://github.com/progscrape/urlnorm)  
40. How to Implement Exponential Backoff with Jitter in Rust \- OneUptime, accessed February 22, 2026, [https://oneuptime.com/blog/post/2026-01-25-exponential-backoff-jitter-rust/view](https://oneuptime.com/blog/post/2026-01-25-exponential-backoff-jitter-rust/view)  
41. Best practices for ethical web crawlers \- AWS Prescriptive Guidance, accessed February 22, 2026, [https://docs.aws.amazon.com/prescriptive-guidance/latest/web-crawling-system-esg-data/best-practices.html](https://docs.aws.amazon.com/prescriptive-guidance/latest/web-crawling-system-esg-data/best-practices.html)  
42. backoff \- Rust \- Docs.rs, accessed February 22, 2026, [https://docs.rs/backoff](https://docs.rs/backoff)  
43. reqwest \- Rust \- Docs.rs, accessed February 22, 2026, [https://docs.rs/reqwest/](https://docs.rs/reqwest/)  
44. \[Rust\] —Web Crawling Like a Boss: Reqwest-rs and Tl-rs duo is Awesome\!\!\!, accessed February 22, 2026, [https://levelup.gitconnected.com/rust-web-crawling-like-a-boss-reqwest-rs-and-tl-rs-duo-is-awesome-af0f0a6b1cc1](https://levelup.gitconnected.com/rust-web-crawling-like-a-boss-reqwest-rs-and-tl-rs-duo-is-awesome-af0f0a6b1cc1)

[image1]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAEoAAAAVCAYAAADhCHhTAAADCElEQVR4Xu2YWchMYRjHHztlTbKFXCGhrJHcuFFSkiWFj+zJekWEIlJC5IJcfHZxI64sF7LvISl7CikXRJIl/P+eM817/jNnzDSnTNP86tc353neOd97nnPO854zZjVqVDrtYAsNVjvN4Cg4FnaWXD66wBuwjSaqlV7wFHwE18PN8BM8Aztmh8VgUa/DWZoAreFj+Ba+M99v89gIs53wFXwKn8Or8XTlscr8YGbAxkG8B7wFX8KWQTzDVvN8A00ELIAf4W/LX9DR5v97sBXez39nO/wGh2oiYqb5Qa6TOK8YXnEjJK4cg3Xm+7grOTIG7tZgpTHe/AAKTZQ9iGNuS3wpfCixfNw3v1J4W3E/WtiNcJLEkmgKu2pQ4F2QKmy+b+BX82Ik0Qr+gh8kfg6elJjCvnc8+jzVvFBHsum/XIQdJJYEV9dLsK8mIubBPRoslxXmE9+rCYFXAMex2YY8M+9RhZgPF0afm5g39u+wUxTjI8Wd6HOxdDNfQPpLnP/rAGwo8bI5b16AOZoQOAGOOx3EGpkfMBt1IY7CPsE2+xz3tTbaZiPflU0XTXd4Ew6ItjmPQ+bzShWeXTZwTnqY5BQWiOOWBTH2AcZ4oIW4J9u8klhg3vKcA/vThNiI4skUa5P57Zx6kQiff9h3aL5lPwN7Ace8tvgz0EDzQvFvEr0t259CDpt/d4p5v2kfT5fEBvgZDtFEmrDncMKFGvl+8zHTJd4zik+UeAh70yINguGWXUV1JS2F5ea3G3sWV9TMbZg6bHyc8GRNRPDh8CdcognzlZDfXamJgBOwnwYjWCB+f4cmioRtIOxJPNlXLLfBpwLf47jkX7Pcy382/AHnSjyEt+M+DUawQNx30qtPnXmh+BxXKothveWubjyey5Z8csqC9/YD86WeV84W+ASehSODcfmohxck1ha+MH82o1/gmtgIhz2S73h8LioFFp5vElqkDFwstmkwLfhuNwhOM//FgCtaMbA/vbfcF90aAs8qfxFYrYkauYwzX57/9f5Vw7wHHdRgNfAHHQWUBiSoN1gAAAAASUVORK5CYII=>

[image2]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAHMAAAAVCAYAAAB17tGhAAADG0lEQVR4Xu2ZWchMYRjHHztlTbKFXCGhrJHcuFFSkiWFj+zJekWEIlJC5IJcfHZxI64sF7LvISl7CikXRJIl/P+eM817/jNzzLjwNc751b9vzv9558x85znv+zzvGbOMjIx00AZqpmZG3dMEGgGNhjpKrBidoGtQKw1k1B09oBPQA2gttBH6AJ2C2ueHxWDir0IzNABaQg+h19Ab8/M2jY0w2w69gB5DT6HL8XDG37DC/IJPgxoGfjfoBvQcah74OTabx+tpIGAe9B76acWTPtL8swda8nkyymAr9AUarIGI6eaJWCM+Zx5n7jDxlSNQjfk5bkuMjIJ2qplROWPNL3LSxWRN5Jib4i+G7otXjLvmM45LKM+jyV8PTRCvFI2hzmoKXE1SBxuWV9Bn84SVogX0A3on/hnouHgK6/DR6PVk82Qeyod/cx5qJ14p2DVfgHprIGIOtEvNNLDM/OLu1oDAmcRxbFBCnpjXzCTmQvOj143Mm6GvUIfI43bmVvS6XLqYN119xedn7YPqi58KzponaZYGBF4kjjsZeA3Mk8LmJonDUK/gmHWX51odHbP52ZEPl01X6DrULzrm9zhg/r1SB2cJmx5e2CESU5hEjlsSeKxL9JiMJO7IMWckbwIu7/wOrJfjYiPKJ5fQDeZLdyoTSbg/ZB2kim05crA2ccxLi+8R+5snk39L0dPy9TLkoPl7J5nXv7bxcEWsgz5CgzSQNlgDeVGTmp+95mOmit898seLH8JauUBNMNTy3bF2yJWw1HxpZQ1lp5xbclMJmwVe1IkaiOAG/zu0SAPmHS7fu1wDAcegPmpGMIl8/zYNlAmX/LBG8oa8ZIVNUWrgc1duN65Y4VI3E/oGzRY/hEvvHjUjmESeu9RjwBrzZHKfWykLoVor7Fr5/1y00jfQfw9rzT3zbQZn4CboEXQaGh6MK0YtdE681tAz870r9QlaFRvhsGbzmSz3jZXAm4NPrDSROdhgbVEzTfBZ7ABoivkvJexUy4H18q0VPjzPqEI4O/hLyEoNZFQnY8y3Bn96XppRJbAm7lcz49/wC3CplAYBIRryAAAAAElFTkSuQmCC>

[image3]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAD4AAAAUCAYAAADV9o4UAAAAnUlEQVR4Xu3VIQ+BURTG8SNJkiAJitnYTGAEkhF9I1VVRIoiGMHMZjYfwJfyt5s8H+Gc899+5T7xvXevWZZlWeavGqp6GKE+XlijLpv7KljhjS2a/3OM5njggI5sIRrhgjOGsoWoiyPumMnmvt+bv+JjQf4AbezxxEI2lw1wsvKVJ7K5bIqblTfdk81lSyvXeYeWbG4bY4OGDlnmoy/B+RIC47QpJAAAAABJRU5ErkJggg==>

[image4]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAE0AAAAVCAYAAAAD1GMqAAAEbUlEQVR4Xu2YeainUxjHv7bs29iXXGRG9hAiWRMRRoY/ZM1aCvnDLhMma5ElkqyDLDGhmGHE1IRsWZNt7Gsiyh6+n99zzr3nPd73d3/XGu6nvt33Ps95t+d9zvOc85PGGeffyrLWorXx/8TC1nbWHtYqla+NVa0nraVrx1/MQtbytfHvZl3rXusVa6p1nvWVNdNaaWRYAwL8hHVY7TALWi9YH1kfWx9YqzVGNMH3lmLsh9bz1uKNEQEf8gvrF+vdytfGhtZF1uq1449yquJhD1a8bGbIesqaZy1R2DMXK/zz1Y4CAvqO4iV3aLoaXKN4hk80+lRfxJqrwYJ2veLeJ1T2ZaxjK9vAXGJ9b21ZOxKHKm56VmVfSpGJ21T2mquscxXXaMtI2Ms6yfrZuq3ydcG4QYK2nHVO+luyrXVnZRuIyYqXubJ2FFCzGPN0ZT/eeqmytfGstYHiGjx8DRl8kzVFMeaopruTWzVY0LqYpt8RNAo3deZbRWC6WFKRAdSRkoesGZWtZgXrMUXR/sma3nT3OF9RTy9XBG1i090JQXsvHW9k7WNtPOLuwRTcwtpN8eFgfusAxXs/qLgfYspnON7POtxaz9ozO05UPCS1pB9MP8a9WdnfUNS0fuxrnZ2OqWvUoZLNrDPS8YuKjzgoBO1L6w5F6TjSek4RCEoH7K147rK87K5obj8oaijHiODAjtYcxSxEj6iYUQ8rLnZENnRwtGLcfYVtAcVNjylsbZA9O6VjMo5OmuEaZB4dmIwkm28p/KPBWGrx2oWNWUHnLafdimqvyZ+qfXreo2bT4Pp80N504YZcbKtiQBsEi3HlhYaSbefC1sYzGumENyoCk/+nJm6fjnM9I1sGhaCRaTXYuVYOJjVzLEGbbX1unWJtolgZsGzpfV1eALUtJTLUAca8r+acZ1rxIPztguwhzTNTFecwDdawLi18VyTfoPUMuoJ2oeJauQ4tlv5vC9pdlQ02VTQYzkEc59kyPNf7NQGygzEHVfa1kp0M6YJCylIjc4jiHGrKddaEwveyxlbPoCto1FnuwzIGyOyuoN2djpnCeelEQnHOrtYFadzwfWjzXGz/bKhgTUXHO652KGoH55LCXbA+26X4n60Z5zyq6F4ZdhvYKexjoStoZA+lJzeDrqDRBHL3p0RNS8fsYvK5QPYz23rk7cjj+u3Cj1b7o/rXGKbstbUxsY7iodYvbGxjeHg6VQlZjH20plJD0Pio5QeYZH1tnVnYeDeun7t45gFFt6UhsRPKC++3rZPTMayp+AjDsIYhsiwfyCjS8TVrlmLF3I8bFFlTQoNhqn2jWAd9p9hxAAV1nkYKNF2bEsGugrFkzesafQuVIWgE+mrFrCGzqT+nKdZiQPOiqHMP/t6f7LC1Yq8717pd8ezwqqIscb3LFEsaSksD9pqbWwcqftkYaro7oZ59pmaD+KegXLBAzi8+FupfcvKPE8Rl5dLxZ8DX5BeR02vHOP2hrVND+v3kM04LbINuro3/NX4Fws8AqGcPGV8AAAAASUVORK5CYII=>

[image5]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAADwAAAAVCAYAAAAaX42MAAADq0lEQVR4Xu2XW6iVVRDHxzRK0EoqNQ0PkmggUSgmiHcfQlMUCnspOaFSQZT4pKh5xAcRQh+0F8lrakE+dAHBLqBQaGgP5gUUxeNd0BATvJfOz1lr7/nG77j3AbHinB/82fubme/71qw9a9baIu2001q6qTpH4/+Fx1QjVa+rngu+MnqpflM9GR0PiUdVz0RjPQxQfac6qGpSLVH9pdqm6lENK8Dk7FK9m66HqI6rzjk1q46ojqkOqJar+qf4yCTVIlWn6CiBH+Oi6rbqRPDVZK7Y4KZJ8WUNqt1ig+3i7JlPxfwdgv0tsYF87WyPqEartqiuq6Y6X4b3cN8r0dECj6t+lVYmzIwzgFejI9EoNoiFwf6EWAUMC3aYIHbP+uhQOqp+Vv2tGhN8L6nmB1stvpJWJDxFbGCfRYeDNUrMnmD/WLU/2DI54bXRkXhWdUWsxOsp3/uxWepMmCZzWnVVLKmW6Kr6R2y9eH5UfRNsmZzwmuhw7BOLGZGue4tVC+u4ew5KvKj6UDVerE/0c75NUk2Yyeuj6ulUYbbYC1d5YwkMgrijwU4jYg2XkRNeHR2Ob8VimtI1zepCso1KNiAOHzvHPLHKeNP5Sfhk+k6C18SewZIpVO5PyTHDG0t4Tyzue2djHd5Qve9snnoSXiH3VgGNzCf8supPsfdl6Asx4VPpO9W4V2wSn88BwN5Fo+LhQ72jBBIlbpazNSTbOGfz1JMwzYaYZc42MdlywpQ712yXJMky5Fd8OvmBhFma9AXOA685XwX2T9YlKttuMgPFYphBtoDMILGB8FlGPQkzOGImO1u+LycMK5MN3RRrhP5UR8KXVH+kmLedrwBrkoD7NSzKh5h3gr1vsvvS8tRqWpQeg2TN8j1DU/IJU8qIJvWBamvyNyU/kDBlz8GJaqS5Fso5s0Hs5rIDAHB6uqX6KDrEBsm9c6IjUSvhvH7jrxETZqIXV9132aja4a6/FOv4QLkziT/IvYehytFspxTXBEwXK5+Zwe6hzD+PxgRlysCZVA/rbKnYszndRd4Qu29sum4U68B+fEwiZZ6hix+SamPjB+IZn1QiHOxp1D5bDIEM5rDYDA13cWWsU20PtnyWZm/npSTWLPZ8yo7Z/0Kqe6+HJsbJDZ1VLRBL+Bexd9HcWGKIiWPfZjL8PUzU76rLyXZG9YIE2KwHi5UX/5Aaiu4WYf2el2Ize9A8JdZggcMIu8u/Bn8G+GfFYaDNwDGQ8qG82gz8u2Fd/me5A8Me4T9B+TKNAAAAAElFTkSuQmCC>

[image6]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABEAAAAYCAYAAAAcYhYyAAABEElEQVR4Xu3SvyuFcRTH8ePHJFeRieiWQVJks7Ew3PIH3MFAVgvDrWtjUCaDgbJISRYGufJPWCXFaCElk4H3cc73cfpORsPzqVc9z+f7fE/PL5EyZf6eTsxh0M87MIV5dKeLyDhq6AldkVMc4E1s2AWa2McrhrGDXTTwgVndmDKDTYziC4/o9zW9C+1eMO2d5hYn4VxWMIZFsQ06NGXEu43QaZ5xnHU/ORR7nPbQLYkNmQzdhHfLoSvygMusOxJ7J3HwFj7l95GL6FfR6etZ/4TzrLvHtR/voS8t1MWG6GdNqXq3GjrdoN2a2Ps6C2uyjTu0hW4B72IXx1yhhRsMxIUKumLh6c0Lz5DYD1rm3+cbWwYurM1cW5kAAAAASUVORK5CYII=>

[image7]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAsAAAAXCAYAAADduLXGAAAA1klEQVR4XmNgGLSAF4i50QXRQTsQfwfi/0BciiaHFaQwQBRboktgA7OB+CsQs6JLYAN3gHg3uiA2IMsAcUINkpg4EFsh8eEglgGi2AaImYC4E4iXAPFFIA5FUgcGc4D4GwMk2KYAsRkQZzFADIhDUgcGIPdeZYBo0oKKqQNxNhBzwBSBQDQDxARrBogb3wDxXGQFyGAmA2qQbQDiW1B2JBC7Q9lgcA2IdyHxVwHxASh7CwNS9LMD8V8gzoEJAIELEL9mgGjC8JwcEDOiiYE8JYomNgrgAACXXCOZ5tyyogAAAABJRU5ErkJggg==>

[image8]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAoAAAAXCAYAAAAyet74AAAAx0lEQVR4Xu3QMQtBURjG8VMUEyVCKaPVYjP5AFKUycBssrDIYpHFoJSNZPERUGyyyidRDAr/4773dpzZYPDUr+553rfO6Sr1zzfjQwFJOftRRMbbkMwwxQUNLNDEAR13KYchUnhijYDMyrgjrA81ZFGRxbws6VSlKxmdOmFjFmSHG0JuEcUDXbcgCeVc2za691v0FSOjW+GonD/gZYwrJrKwRx9Bc0nnjK18xxAxZl7iyrm2Zw/sDJSz2ELamn1kLpaoW7NfyAtnfCMHOBvooQAAAABJRU5ErkJggg==>

[image9]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAmwAAAAiCAYAAADiWIUQAAAC/klEQVR4Xu3dS6i1UxgH8OWSS0REuSsGIooiSiKXkIgoA0WhlJTCRCTXUnKLUmYUMkOUciklAwZGcikxJEoSuRSep7Ve5/U43+lsZ5/j6/P71b93rWftwR4+rbXevVsDAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAYHGnRp6vxW3YuRYW8EnkploEAGB9Tq+FVVxTC2vYrxbCj5HdahEAYHt2Zy38Rw5tvZG6O3Js5OHIxZEHZp9Jj43nHpELWv/+j0Sujlw6fWg4LrJXqb05nvdE9pwvAABsj35ovSlK2SQ9GPl9ZXlT3FFy46g/Hbmq9e+xa+TeUd89ctAYp8PG85TIh2P88nh+G9lpjCdfl/kHkRMix5c6AMCWyOZmvXI364paDL/UwhbIhiyPKq9t/Vj0zMhPY+2p1nfP0knjOclm88TIl2N+SeTmv1a7XSKXjfGV4/ls6/fY9h1zAIANyYbj7DHOnadjZmtzL0W+j7xe6qeV+SQbo2x2qmU3bHkkeVZbu5nMxuvV1ne97hq1d8bz/sg5Y3zfeKbcSdsnckPkyVHL49M8Bq2m9TxmTde13hwCACzFQ60fA77RetNzefvnztBRZX575MXIM60fO65maoyqZTVsuYP3duTwMX9vtvZvnBc5vxbX6ZtaAABYtifaypuNuct0xmxtcm7k8chFpb53mU9eqYVhrYYt75i9sI1UH0d+jjzX+tHrRn6KI+1fCwvIe3neDAUANs0B7e8vAvw2G0/yaHDeEH3aelOXF/Nfm9Xn8niwXtBPazVsi/iq9V2+yWo/sbFV3q0FAIBlyiPQP2bz92fjyZFlnken+QblZ5FDytrk+rbyxuXcr63flduofOvzizE+ua3+vbdKvmQAALBpPoq8FTmwbexYcDW31cImOLoWtljeoZv/LAgAwFId0fru2i11YUly1+vRWtzBfFcLAADLdGvrx3nr/Y/NRR0c+bwWdyB5r+/CWgQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAOB/6U8XEFq106lKXQAAAABJRU5ErkJggg==>

[image10]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAwAAAAXCAYAAAA/ZK6/AAAAuUlEQVR4Xu3PLw9BURzG8Z+/wdjYBBvT6GiKCZrpJgiCqTRB9RZUQRFMEDQvQrEJpgvegPG99xz8dooq3Gf7hPM8557tigT5p0TQRkGdq2gi9r6ks8Ead7SwxRQL3JD5XhWpY4Yynrgga7ek7Ub27GeAEnp2bKjN671uqLpPztg53QFHp/OTF/PSRHU5PMT8SxRLtUlXzAcV1XVsV0MfY7XJHCeEVJfGFXusEFebpJDQhU0YRbcM8isvgIIcyrJO7pgAAAAASUVORK5CYII=>

[image11]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAgAAAAZCAYAAAAMhW+1AAAAw0lEQVR4XmNgoDvgAWJedEEQKATir0D8H4h70OTgwIcBoiAAXQIGGoH4HxALo0vAwEEgvoguCAMcQPwDiCcAsTIQBwExO7ICRwaI/ReAeCIQ10DZujAFDQwQ+6NgAkCwD4jnI3MuI+TA4BAQP4Fx8CpgA+JvQDwFRZqB4S0QXwExdBggDgxDkjSAii0AcbShHCMkBaBAA3lbDcRhBuLPQBwPlQSFw0cgzofywQAUDzeAeC4Q3wTiUGRJGOAGYkV0weEBAKApJyIzhIFcAAAAAElFTkSuQmCC>

[image12]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAoAAAAYCAYAAADDLGwtAAAA3UlEQVR4XuWQz+rBURDFZyGy8+eXYiErSwsbdiTFQvECtvIQdmKjpCjqV17C3oJkxyvIE9hYUOKMubem8QLKqU/fe885zdy+RF+tEIhZU6sELuAJ9ib7EE/i4sgGVlWSYsMGVgPwABEbsPKg6M47cFDZW2mwBSvQc987GOtSEpzBQnlDkvc1lUdLcANx5fVJ3hf1RsIZvFZrA47aqJGs4AleYZINE+VRmaRYV17FeS1QAF02g+AK2q70R7KSixkwAzmXUQecwBz8k0zkv7AGU1/y4ndl1T0AUur+g3oBR8kohfRh6YEAAAAASUVORK5CYII=>

[image13]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAYCAYAAADzoH0MAAABMElEQVR4Xu2SvytGYRTHj5QfYZDVImXxI6wWdoMUg0VmCasoUiaFskkku/9AsVqIgRWrJJEJn/Oe573veU/3rdck5VOf7vN8z733+SnyCwzjBe6F/Ecc41wMlWZsiWEOD9jng0V8wy/c9AXHEG7gDD5hTXlZZFTsB2OxACu4ntoHeOJqGWv4iW0hHxCbXWPqH+FCqVziDK9iCMt46vr32O/6BRrwA7exE8exPtXmcSu1deOesQOnUlZgRGz9l7gjNqq2e7FdbN2zuITnYgPpoBmrYuv3f9Vp64dFWtNTd784uwx9+TpkOtJjyHKpw3fcDbme9U3IcukRW/+ky3SXNTt0WUW6xV4edJneCT2VLpdVpBZfcTr19RhfxI6vavQa3+I+3uFEebk6msQuyD9/jm8PcDZHxN2biQAAAABJRU5ErkJggg==>

[image14]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAE8AAAAYCAYAAAC7v6DJAAAEuklEQVR4Xu2YZ4gmRRCGX3POigldBE/FjDkfKgjmgAFBZU2YQEXMih6IiAlBFMGcDgNmMQsKZtQfZhHlMGfMOb6PNe3XWzuzt8fttwb2gZflq+rtmamurq4ZaYIJ/i8sYs2Tjf9BeI55s3FGmMvawtreWjr52ljGesZaKDsSc1iLZ+M4Mn+jkVjVetyaLzumx8rWXdar1hTrLOtr6wFryd6wIRDop639s6NiKesL6w/r3eQbD46zflJc/+Tka+N8RRxmzY4uTrI+svazZq/sA9az1jS1r9p5Cv8s2ZEgyKxov4M3aC2bjWZ9jT54C1ofWwdnRxsXKFZmg+xoGFRc+PRk5yJk5ibJ3sVU9T94D1urZaNZVKMPHhxgvWPNmR01uygmvTg7KqhpjHku2Y+yXk62kbhe/Q3eYtZXag/ewpqx4M1m/Wztmh0FCvz71g+KAHWxgPW7om7VPGTdkWw1PMw21uaKk7greMy/l2KbrJ3s6ynmYFfwQOtYO1vLVeOA8nKPIkDbWZM0dPvyrCV41LK1rC0Vh1gXb1gXZWPhGMWEl2ZHgm3JuLeS/U1FzWuDrCTYV1pHW7dbr2h48CZbH1gnWHsqas05jW9rxeHFtV+w7lccYuda3zd/S629oRr7hOKQO7PxAQuBjxJ1i3WGdY0iU1epxtXcq7huK9QHJjwoOxKHKMbdXdlKWh9a2Qp7KMZvVdnIPG6kDt4S1peKIBR2Uvzvhs1vrsOY1xWZXDhSMe6IynZ4Y2vbtiV43PPyjY0M/NC6rAxKXGh9mo1Aupbju9xoFwSNcWRQYaCxkR2Z1zQ8S4F56uCdqphjW0Vfhegrf7NOrMbRBeTyQOvE/75d2UYTvBuTnd3wWLIVWCDuZVjLQutAHUNtLUiBG2HMe9bclZ3aw83wt4bunAuydTI5eDwIc9xmXZu0dzWOupyDB2WbcupDCd7qf4/oUYKXy8xLihaqjX0Vz9564pIdTDjSYUFdYAwT1azQ2HdPdl5vsD+V7EDwWIQC9ZCxm1W2NrqCx0n/q3pvNiV4aza/d1CvZy0HxtnN7wKlpG2h4TTFtVthhZmQQt0Gbw3cHOmbKStZb68CLQ2HQIbg1Tezm2KOwyobUNs2rX63BY9s+9G6tbJRf5mvnNj3qXealkXNwXtR3cEjPl1Z+Vd94UQkS+piDAdav2jkLpssujwbFStOuhOcwhrW54qmuv6AcKeGHgacnrQHJXuA4LEYpT1hzCWKmr1xGaSo3QSoXPeRyld61dzPcu3n1VLXzJOKAHbCawvRp+0gw1gZ+psHNf3tdLX1aLIVBq3PFNnGDVyl+HjAA3ynaM6B5pX2gxJykyJbeEWsIXg8IH6Cxv2yZenVMlcoEuJm9UrKseot3DeKQ4ZOYFpjQyxOfsP6xDo+2YZBXVjX2kfxJWVgqLsTbo6jvD5Iatgy9FBsGaBOMjdbLr8LU5NWSrZCvW3JoBUVLUwXLMjMfh4jy8lsPmr0BVKdE++U7BhjaFXY3uPJVMUbUV/ZUbEV2r5kzCzUQZrmbxXblJ6yK8vHEg4rtv6k7OgHNLvXZeMYMFlRV6ljiNrZj0WqYTfxiY3GfdyYon/2K/FYsZHic9QEE/zL+BOxuRa8Ks7fQQAAAABJRU5ErkJggg==>

[image15]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAE8AAAAYCAYAAAC7v6DJAAAEpklEQVR4Xu2YV4hkVRCGf3N2TZh1kDVhRjFgAgOirosBw4OBMYsPKmJWdFFETAiiL6ZV12VVhBUjBgyYURGziLqYM6IiYrY+6x67uvrcnp2Rlp1lPvhpbtXp2/fUqVN1bksTTDC/sLxpiWwchzCPJbNxNCxm2tk0xbRa8tVY3fSCaVJ2DJBFTCtl4wgs3agfG5meNi2VHSOxgeke01umaaZLTD+YHjKt0hnWBYF+3nRkdoyBTUyXm9bMjsCqpu9Mf5k+Tr42Tjf9Iv/OOclX40p5HBbMjjbONn1hOsK0cLAPmV40zVF91a6Q+xfIjjEwXT7BU7IjwYKRHbXgDZvWyEZja8198JY1fWk6NjtqXCVfmW2yo2FY/sMXJDs/QmZun+xjZUXTRc3nSMxUPXiPmjbORmMFzX3w4CjTR6ZFsyOyn/ym12ZHgJrGmJeS/WTTG8n2f3GbeoNH0L9XPXjLaXTBW8j0q2n/7ChQ4D81/SwPUBvLmP6U15rII6a7k41s5F7USD4p6nxSq9DazTi2VrHR4ZgcW2tP1SdPYPYw7STv6jl4lJf75QHa27Seurcvcy3Bo5ZtbtpF3njaeNd0TTYWTpXf8LrsSLAtGfd+sr8nr3kROvQr8vGsHPXrDtNP8gVgWwGBZ8zvpuNM+8rvXysPZDgLd5P8frNNb6o7eLPkjY7vPyNvchcHPwmAjxJ1l7w83CLP1A3DuMgDplezscBEuOEx2ZE4Xj7u3mAraX1CsBVoHtSLJ4LtTHldZRKFD0y7heuV1Ru8gxrbrsFG5jGpvG1PlI+tZW4JHs9csp8M/Nx0fRmUuNr0dTYC6Vra97bJlyFojItdcKixxclHLjP9oc45kZrK+EOb63XlGRmhm+fgva3ejAeeaSzBuz3ZyeCnkq1wknwOPUcW2j3bCNWOIAUehDGfmBYP9i3lD8NnjS3kfh6ALOUs+JjpvsZP7aFZRTjZx+BxzcOzDTP9gsd5MVOCl8vM6/JjT43D5XOvdtxSY/o1C+oCY7hRZJ3GfmCyR6hBBG1304XycxPbhuL/pHwBI2zHGDwaCdfP/TuiA8FjQSMleJs11/uoc2YtDePS5rrA9q8tDpwvb6hVbpXf8ODsaOCtgYJO9mTKSp6VHYHz5GPIuPXlZy2CRy25MYwr5OABx6PPwnWB4OWJUX/5PlkPD6rTTctC5OC9pvbgEZ+2rPynHtHFWNl8MD3a9Jv6n7JZ+RuyMTBZ/sC8gRR47WEr1Golz8B4srRA9jD+gGDb1PSt/IAe/4ygdvP9Mvbx4GN34cvn2XdML6tS14xn5QFshbMV0efYQYaxMpxvHjbtGMbVuFndHbUGfxjwblk4RN7h8sPSjEpA+Cy1EYZN38izjclMl9+XYHAEirWTjCYh7lSnpJymzr1/NH0o795zGhsiu/Mb1lemM5KtB+rCVqbD5Oe0oW53KzwcrTw2kgzHj+inefSrsW2w/TiPsf2AmstzcijP79UcuP/r32NryU8jHOIHAtlDUzg3O+YDZsrfYgbKVPlWqP2TMV7ZQb71ecUbOHTVGdk4TmE30eD2yo5BMk2j/2d3XmQ7+d9RE0wwj/E37GkPJez4nCQAAAAASUVORK5CYII=>