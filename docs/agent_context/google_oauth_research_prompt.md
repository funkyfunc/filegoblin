# **Comprehensive Architectural Analysis of Google OAuth 2.0 Integration for Statically-Linked Command-Line Utilities**

The integration of robust, persistent authentication within a statically-linked, zero-dependency command-line interface (CLI) such as filegoblin requires a sophisticated understanding of the OAuth 2.0 protocol, specifically the Proof Key for Code Exchange (PKCE) extension. As these tools operate in untrusted environments where client secrets cannot be securely embedded, the transition from unauthenticated public access to authenticated Workspace resource ingestion necessitates a multi-layered strategy involving local loopback servers, cryptographic handshakes, and strategic token management. This report evaluates the technical requirements, security considerations, and implementation pathways for enabling seamless Google Docs, Drive, and Gemini content acquisition via the Google Identity platform.

## **1\. Architectural Foundation of OAuth 2.0 PKCE for Public Clients**

Public clients, defined in the OAuth 2.0 specification as applications capable of being decompiled or inspected by end-users, are fundamentally unable to maintain the confidentiality of a client secret.1 For a Rust-based tool like filegoblin, which is distributed as a statically-linked binary, embedding a secret would expose the application to impersonation risks. Consequently, the Proof Key for Code Exchange (PKCE, pronounced "pixie") protocol is mandatory for securing the authorization code grant flow.2

### **1.1 The Cryptographic Handshake and Mathematical Underpinnings**

The PKCE flow introduces a dynamic secret generated for every authorization request, effectively neutralizing the threat of authorization code interception. The process begins with the generation of a Code Verifier, a high-entropy cryptographic random string. According to the Google Identity standards, this string must be between 43 and 128 characters and consist of a specific set of unreserved characters: \[A-Z\], \[a-z\], \[0-9\], \-, ., \_, and \~.3

From this verifier, a Code Challenge is derived using the ![][image1] transformation. The mathematical relationship is defined as the Base64URL encoding of the SHA-256 hash of the verifier, represented as:

![][image2]  
The use of the ![][image1] method is strongly recommended over the "plain" method, as the latter provides no protection against certain classes of interception attacks where the attacker can view the initial request.3 By sending the challenge to the authorization server and only revealing the verifier during the token exchange phase, the client proves its identity through the possession of the original random value.2

### **1.2 Authorization and Token Exchange Endpoints**

The implementation must interface with Google’s global OAuth 2.0 endpoints. These services are designed to handle high-concurrency requests and provide the necessary metadata for token lifecycle management.

| Endpoint Function | URL | Protocol Method | Requirement |
| :---- | :---- | :---- | :---- |
| Authorization Server | https://accounts.google.com/o/oauth2/v2/auth | GET | Initiates browser-based consent.3 |
| Token Exchange | https://oauth2.googleapis.com/token | POST | Swaps code for access/refresh tokens.3 |
| Token Revocation | https://oauth2.googleapis.com/revoke | POST | Programmatically removes app access.3 |
| Discovery Document | https://accounts.google.com/.well-known/openid-configuration | GET | Provides automated endpoint discovery. |

For filegoblin, the authorization request must include the code\_challenge and code\_challenge\_method=S256 parameters to satisfy the PKCE requirement.3 Furthermore, the access\_type=offline parameter is critical for receiving a refresh token, enabling the "silent" authentication pattern required for subsequent runs without user interaction.3

## **2\. Redirection Mechanisms and Local Environment Integration**

A significant challenge for CLI applications is receiving the authorization code after the user completes the consent process in the system browser. Unlike web applications with fixed callback URLs, a local binary must coordinate with the operating system to capture the incoming HTTP redirect.

### **2.1 The Loopback IP Address Pattern**

Google supports the loopback IP address (macOS, Linux, and Windows desktop) as a primary redirect method for native applications.3 The CLI tool instantiates a temporary HTTP listener on a random, ephemeral port. This is often implemented in Rust using a low-level networking crate or a lightweight asynchronous server. The redirect\_uri is constructed using the local address: http://127.0.0.1:PORT/ or http://\[::1\]:PORT/.3

Evidence from developer implementations suggests that using the numeric IP 127.0.0.1 is more stable than the string localhost, as some environments may have misconfigured /etc/hosts files or DNS resolvers that introduce latency or failure.5 Google’s authorization server performs a string-exact match on the redirect URI, meaning any variation in port or path between the registration in the Google Cloud Console and the runtime request will trigger a redirect\_uri\_mismatch error.7

### **2.2 Handling the Browser-to-CLI Handover**

The user experience for gobble \--google-login follows a deterministic sequence:

1. **Generation:** The tool generates the PKCE verifier/challenge and a random state string to prevent Cross-Site Request Forgery (CSRF).  
2. **Listener Initiation:** A TCP listener binds to 127.0.0.1:0. The operating system assigns a random available port.  
3. **Redirection:** The tool constructs the authorization URL and invokes the system’s default browser.  
4. **Consent:** The user authenticates with Google and grants permissions.  
5. **Callback:** Google redirects the browser to the loopback URI. The CLI listener captures the code and state parameters from the GET request query string.  
6. **Exchange:** The tool performs a background POST request to the token endpoint, providing the code and the code\_verifier.2

This "no-automation" flow is preferred because it respects the security boundaries of the system browser, where the user's Google session is already active, thus avoiding the need for the tool to handle raw passwords.2

## **3\. Scope Management and Minimum Privilege Analysis**

To ingest diverse content types for LLM optimization, filegoblin requires specific permissions within the Google Workspace ecosystem. Selecting the most restrictive scopes is necessary to reduce security surface area and minimize friction during the Google app verification process.

### **3.1 Drive and Docs Read Access**

Access to Google Drive and Google Docs is governed by a set of scopes that range from per-file access to full drive management. For a tool designed to read any URL provided by the user, the "readonly" variants are the most appropriate.

| Resource Type | Recommended Scope String | Access Detail |
| :---- | :---- | :---- |
| Google Docs | https://www.googleapis.com/auth/documents.readonly | Read-only access to documents.9 |
| Google Drive | https://www.googleapis.com/auth/drive.readonly | Read-only access to all files.9 |
| Metadata | https://www.googleapis.com/auth/drive.metadata.readonly | Identify file types and attributes.9 |
| Specific Files | https://www.googleapis.com/auth/drive.file | Access only to files used with the app.9 |

The drive.readonly scope is essentially a superset that allows reading both native Google Workspace files and uploaded binary blobs (PDFs, images, etc.).9 It is important to note that these scopes are categorized as "Restricted" by Google.9

### **3.2 The Verification and Review Barrier**

Applications requesting "Sensitive" or "Restricted" scopes must undergo a verification process to remove the "Unverified App" warning screen.9 For a production-grade application, this involves a manual review by Google’s security team, potentially including a third-party security assessment known as CASA (Cloud App Security Assessment).11

However, for personal use or internal tools, Google allows applications to remain in "Testing" mode or "Unverified Production" status. In these states, the app is limited to 100 users, and users must acknowledge a security warning to proceed.14 For an open-source CLI tool, the most common pattern is for the developer to provide their own client credentials or allow users to generate a personal client\_id and client\_secret to avoid the bureaucratic overhead of full verification.15

## **4\. Free Tier Quotas and Registration Policies**

Google Cloud Platform provides a generous free tier that is well-suited for a CLI utility. Understanding the limits of this tier is essential for ensuring that filegoblin remains functional under heavy use without incurring costs.

### **4.1 Usage Limits and Quotas**

Quotas for the Google Drive and Docs APIs are typically calculated on a per-project and per-user basis. These limits protect the integrity of the Workspace infrastructure.

| Service Category | Quota Metric | Free Tier Limit |
| :---- | :---- | :---- |
| Drive API (General) | Requests per Day | 500,000 17 |
| Drive API (Concurrency) | Queries per 60 Seconds | 12,000 18 |
| Workspace Export | File Size Limit | 10 MB per request 11 |
| Drive Export (User) | Queries per 60 Seconds per User | 12,000 18 |

The 10 MB export limit for the files.export method is a critical constraint.11 Large Google Docs that exceed this size cannot be exported as a single Markdown or PDF file via the API. For such cases, the CLI tool must either warn the user or attempt a more complex multi-part extraction, though the latter is not natively supported by the standard export endpoint.13

### **4.2 The Lifecycle of a "Testing" Project**

When a project is first created, it resides in "Testing" mode. This status imposes a major restriction: any issued refresh tokens will expire in 7 days.20 This "7-day curse" often leads to broken automation pipelines where the CLI tool suddenly prompts for re-authentication every week.15

To achieve persistent, "silent" authentication, the application status must be changed to "Production" in the Google Cloud Console. Even without completing the full verification process, moving to Production removes the 7-day expiration limit on refresh tokens, allowing them to remain valid indefinitely (or until manually revoked by the user or invalidated by password changes).15

## **5\. Content Extraction and Format Conversion Mechanisms**

The primary objective of filegoblin is the conversion of cloud resources into LLM-optimized Markdown. This requires distinct logic for different resource types hosted on Google’s infrastructure.

### **5.1 Exporting Google Docs to Markdown**

Google Docs are not stored as files in the traditional sense but as structured database entries. To retrieve them in a format like Markdown, the application must use the files.export method of the Drive API v3.11

Historically, Google Docs did not natively support Markdown export, requiring developers to parse the complex JSON returned by the Docs API get method.24 However, as of early 2024, the Drive API now supports native Markdown export.25 The request is performed via a simple authenticated GET:

GET https://www.googleapis.com/drive/v3/files/{fileId}/export?mimeType=text/markdown 13

The response body contains the raw Markdown content. This native conversion is generally more reliable than third-party libraries because it handles Google’s proprietary internal formatting (like tables and nested lists) at the source.25

### **5.2 Detecting and Routing Drive Files**

When a user provides a drive.google.com/file/d/\<id\> link, the tool must determine if the target is a native Google Workspace document or a standard binary file (e.g., an uploaded image or zip). This is achieved by first fetching the file’s metadata.27

| MIME Type Category | Format | Action Path |
| :---- | :---- | :---- |
| application/vnd.google-apps.\* | Doc, Sheet, Slide | Route to files.export.28 |
| Standard Binary | PDF, JPEG, TXT | Route to files.get?alt=media.19 |
| Folder | vnd.google-apps.folder | Recursive crawl of children.28 |

The alt=media parameter is a universal Google REST API signal that the request is for the raw content bytes rather than the JSON metadata.19 This distinction allows filegoblin to handle both cloud-native documents and traditional files with the same URL-based entry point.

### **5.3 Programmatic Access to Gemini Share Links**

Ingesting Gemini conversation share links (e.g., gemini.google.com/share/\<id\>) represents the most significant technical challenge. Unlike Docs or Drive files, Gemini share pages are currently rendered as static, public HTML snapshots intended for browser viewing.31 There is no documented API that allows a developer to fetch the underlying conversation JSON of a share link using an OAuth token.31

While the Gemini API (Generative Language API) provides tools like URL Context that allow a model to "read" a webpage, these tools explicitly exclude Workspace files and login-gated content.34 Since filegoblin intends to use the user's login to access restricted content, it must effectively act as an authenticated scraper. By passing the stored OAuth access token (or associated session cookies) in a standard HTTP GET request to the share URL, the tool can attempt to fetch the raw HTML and parse the conversation.35 However, this is a fragile approach compared to official APIs and may be subject to changes in Google’s frontend architecture.

## **6\. Token Persistence and Silent Refreshment**

For a CLI tool, the credential storage mechanism must balance security with convenience. filegoblin stores tokens in \~/.config/filegoblin/credentials.json, following the established pattern of its Twitter/X integration.

### **6.1 Access Token Lifespan and Refresh Logic**

Google access tokens typically have a Time-to-Live (TTL) of 3,600 seconds (one hour).2 To fulfill the requirement for "silent" use, the tool must monitor the expiration time and perform a refresh before the token becomes invalid.

The refresh operation is a background POST request:

| Parameter | Value |
| :---- | :---- |
| client\_id | Your GCP Client ID |
| grant\_type | refresh\_token |
| refresh\_token | The stored offline token.3 |

This request returns a new access token and, occasionally, a new refresh token (token rotation).21 If the refresh token fails with an invalid\_grant error, it signifies that the user has revoked access or that the token has expired due to policy changes.21 At this point, the tool must revert to its unauthenticated state and prompt the user to run \--google-login.21

### **6.2 Security of Stored Credentials**

Since filegoblin is a statically-linked binary, it must take care in how it persists these tokens. On Unix-like systems, the tool should ensure the credentials.json file is created with restricted permissions (chmod 600\) to prevent other local users from accessing the tokens. While a client secret is required for some grant types, in the PKCE flow for a public client, the client secret is not used during the exchange, which reduces the impact if the binary is decompiled.2 The real security rests in the refresh token, which serves as a long-term key to the user's data.

## **7\. Implementation Roadmap for the Rust CLI**

The transition from the existing Twitter/X OAuth implementation to Google OAuth 2.0 requires careful integration with Rust’s asynchronous ecosystem and HTTP client libraries.

### **7.1 Networking and Static Linking**

Because filegoblin is statically linked, dependencies must be chosen that do not rely on dynamic system libraries (like OpenSSL). Utilizing crates like rustls for TLS and tokio for the asynchronous runtime ensures that the binary remains portable across different Linux distributions without shared library conflicts.

The loopback server can be implemented using a minimal hyper or axum instance that binds to port 0 to let the OS choose an available port. This ensures the tool does not fail if another application is already using a standard port like 8080\.3

### **7.2 The Graceful Degradation Logic**

The logic for gobble \<URL\> must be non-intrusive. The pseudo-sequence is as follows:

1. **Check Context:** Does the URL belong to Google (Docs, Drive, Gemini)?  
2. **Attempt Local Auth:** Read credentials.json. If an access token exists and is valid, use it.  
3. **Silent Refresh:** If the token is expired, attempt to use the refresh token.  
4. **Fallback to Public:** If no auth is available, try an unauthenticated GET. Many Google Docs are shared with "Anyone with the link can view."  
5. **Interactive Prompt:** If the public request returns a 401/403 (Auth Wall), surface the message: "Try gobble \--google-login for authenticated access."

This pattern ensures that public content remains accessible without friction while private content is only a single command away from being unlocked.

## **8\. Analyzing Service-Specific Edge Cases**

As a domain expert, one must anticipate the nuances of the Google API that can lead to runtime failures in a CLI environment.

### **8.1 The 100-User Cap and Verification Warnings**

For an unverified application, Google enforces a "User Cap" of 100 unique authorizations.14 This cap applies to the entire lifetime of the project and cannot be reset. If filegoblin gains wide adoption, the developer must eventually seek formal verification to avoid reaching this limit. Furthermore, the "Unverified App" screen can be "scary" to users; providing clear documentation that explains this is a normal part of open-source local-first software is necessary to build trust.15

### **8.2 Quota Management for LLM Workflows**

LLM-based tools often perform high-frequency requests when chunking or analyzing large repositories. While the Drive API's 500,000 requests-per-day limit is massive, the 10-queries-per-second (QPS) limit per account can be hit by an aggressive multi-threaded tool.17 filegoblin should implement a simple rate-limiter or exponential backoff strategy when it encounters HTTP 429 "Too Many Requests" errors to maintain reliability.40

### **8.3 Handling Revocation and Password Changes**

A common source of "silent" failures is token revocation due to security events. If a user changes their Google password, all tokens with Gmail scopes are automatically revoked.21 While filegoblin likely does not need Gmail scopes, general security heuristics in Google’s backend may occasionally flag and revoke tokens for "unusual activity".21 The CLI tool must handle these events by clearing the local cache and informing the user that re-authentication is required.

## **9\. Conclusion: Strategic Advantages of Google OAuth Integration**

The addition of \--google-login to the filegoblin ecosystem provides a high-value bridge between the unstructured web and the structured data environments of modern organizations. By adhering to the PKCE protocol, utilizing the native loopback redirection pattern, and leveraging the newly available native Markdown export methods, the tool can offer a frictionless ingestion experience.

The implementation of "silent" persistent authentication via refresh tokens in Production mode effectively solves the 7-day expiration issue seen in early-stage development projects. While challenges remain regarding the programmatic ingestion of Gemini share links due to the lack of an official API, the combination of authenticated scraping and the model's own URL reasoning capabilities offers a robust path forward. Ultimately, this integration allows filegoblin to operate as a professional-grade bridge for LLM data pipelines, moving beyond public scraping into the realm of enterprise-ready cloud resource analysis.

---

**Key Data Summary for Implementation**

| Component | Technical Value | Reference |
| :---- | :---- | :---- |
| **Auth Endpoint** | https://accounts.google.com/o/oauth2/v2/auth | 3 |
| **Token Endpoint** | https://oauth2.googleapis.com/token | 3 |
| **Redirect URI** | http://127.0.0.1:\<port\> | 3 |
| **Docs Scope** | .../auth/documents.readonly | 9 |
| **Drive Scope** | .../auth/drive.readonly | 9 |
| **Access TTL** | 3,600 Seconds (1 hour) | 2 |
| **Export Limit** | 10 MB | 11 |
| **Daily Quota** | 500,000 Requests | 17 |
| **PKCE Verifier** | 43-128 Characters | 3 |
| **PKCE Method** | S256 | 3 |

This technical strategy provides the necessary detail to implement a production-ready Google login flow within the constraints of a Rust-based, zero-dependency CLI application.

#### **Works cited**

1. Securing Your OAuth 2.0 Flow with PKCE: A Practical Guide with Go \- Medium, accessed March 9, 2026, [https://medium.com/@sanhdoan/securing-your-oauth-2-0-flow-with-pkce-a-practical-guide-with-go-4cd5ec72044b](https://medium.com/@sanhdoan/securing-your-oauth-2-0-flow-with-pkce-a-practical-guide-with-go-4cd5ec72044b)  
2. Implement the OAuth 2.0 Authorization Code with PKCE Flow | Okta Developer, accessed March 9, 2026, [https://developer.okta.com/blog/2019/08/22/okta-authjs-pkce](https://developer.okta.com/blog/2019/08/22/okta-authjs-pkce)  
3. OAuth 2.0 for iOS & Desktop Apps \- Google for Developers, accessed March 9, 2026, [https://developers.google.com/identity/protocols/oauth2/native-app](https://developers.google.com/identity/protocols/oauth2/native-app)  
4. Using OAuth 2.0 for Web Server Applications | Authorization \- Google for Developers, accessed March 9, 2026, [https://developers.google.com/identity/protocols/oauth2/web-server](https://developers.google.com/identity/protocols/oauth2/web-server)  
5. \[SOLVED\] missing redirect url Google OAuth web \- Threads \- Appwrite, accessed March 9, 2026, [https://appwrite.io/threads/1100419020190122014](https://appwrite.io/threads/1100419020190122014)  
6. Why are the redirect URIs configured to 127.0.0.1 instead of localhost? \- Stack Overflow, accessed March 9, 2026, [https://stackoverflow.com/questions/78019306/why-are-the-redirect-uris-configured-to-127-0-0-1-instead-of-localhost](https://stackoverflow.com/questions/78019306/why-are-the-redirect-uris-configured-to-127-0-0-1-instead-of-localhost)  
7. How to Fix 'Invalid Redirect URI' OAuth2 Errors \- OneUptime, accessed March 9, 2026, [https://oneuptime.com/blog/post/2026-01-24-fix-invalid-redirect-uri-oauth2/view](https://oneuptime.com/blog/post/2026-01-24-fix-invalid-redirect-uri-oauth2/view)  
8. Manage OAuth Clients \- Google Cloud Platform Console Help, accessed March 9, 2026, [https://support.google.com/cloud/answer/15549257?hl=en](https://support.google.com/cloud/answer/15549257?hl=en)  
9. OAuth 2.0 Scopes for Google APIs | Google for Developers, accessed March 9, 2026, [https://developers.google.com/identity/protocols/oauth2/scopes](https://developers.google.com/identity/protocols/oauth2/scopes)  
10. Download and export files | Google Drive | Google for Developers, accessed March 9, 2026, [https://developers.google.com/drive/api/guides/manage-downloads\#export\_google\_workspace\_documents](https://developers.google.com/drive/api/guides/manage-downloads#export_google_workspace_documents)  
11. Method: files.export | Google Drive | Google for Developers, accessed March 9, 2026, [https://developers.google.com/drive/api/reference/rest/v3/files/export](https://developers.google.com/drive/api/reference/rest/v3/files/export)  
12. Manage OAuth App Branding \- Google Cloud Platform Console Help, accessed March 9, 2026, [https://support.google.com/cloud/answer/10311615?hl=en\#zippy=%2Ctesting-publishing-status%2Cunverified-app-limits](https://support.google.com/cloud/answer/10311615?hl=en#zippy=%2Ctesting-publishing-status%2Cunverified-app-limits)  
13. Method: files.export | Google Drive, accessed March 9, 2026, [https://developers.google.com/workspace/drive/api/reference/rest/v3/files/export](https://developers.google.com/workspace/drive/api/reference/rest/v3/files/export)  
14. Manage App Audience \- Google Cloud Platform Console Help, accessed March 9, 2026, [https://support.google.com/cloud/answer/15549945?hl=en](https://support.google.com/cloud/answer/15549945?hl=en)  
15. Your Google Drive Connection Dies Every 7 Days? Here is the Permanent Fix. \- Reddit, accessed March 9, 2026, [https://www.reddit.com/r/n8n/comments/1p64bfq/your\_google\_drive\_connection\_dies\_every\_7\_days/](https://www.reddit.com/r/n8n/comments/1p64bfq/your_google_drive_connection_dies_every_7_days/)  
16. 7-day reconnect of Google Drive oAuth \- Help and Support \- rclone forum, accessed March 9, 2026, [https://forum.rclone.org/t/7-day-reconnect-of-google-drive-oauth/50148](https://forum.rclone.org/t/7-day-reconnect-of-google-drive-oauth/50148)  
17. Usage limits and quotas | Admin console \- Google for Developers, accessed March 9, 2026, [https://developers.google.com/workspace/admin/data-transfer/v1/limits](https://developers.google.com/workspace/admin/data-transfer/v1/limits)  
18. Usage limits | Google Drive, accessed March 9, 2026, [https://developers.google.com/workspace/drive/api/guides/limits](https://developers.google.com/workspace/drive/api/guides/limits)  
19. Download and export files | Google Drive, accessed March 9, 2026, [https://developers.google.com/workspace/drive/api/guides/manage-downloads](https://developers.google.com/workspace/drive/api/guides/manage-downloads)  
20. accessed March 9, 2026, [https://developers.google.com/identity/protocols/oauth2\#:\~:text=A%20Google%20Cloud%20Platform%20project,.email%2C%20userinfo.profile%2C](https://developers.google.com/identity/protocols/oauth2#:~:text=A%20Google%20Cloud%20Platform%20project,.email%2C%20userinfo.profile%2C)  
21. Google OAuth invalid grant: Token has been expired or revoked — What it means & how to fix it | Nango Blog, accessed March 9, 2026, [https://nango.dev/blog/google-oauth-invalid-grant-token-has-been-expired-or-revoked](https://nango.dev/blog/google-oauth-invalid-grant-token-has-been-expired-or-revoked)  
22. Refresh Token expire in 7 days \- Google Ads Community, accessed March 9, 2026, [https://support.google.com/google-ads/thread/321488696/refresh-token-expire-in-7-days?hl=en](https://support.google.com/google-ads/thread/321488696/refresh-token-expire-in-7-days?hl=en)  
23. OAuth2 token expires every 7 days for Google Drive (n8n Cloud, private Google account) – how to make it persistent? \- n8n Community, accessed March 9, 2026, [https://community.n8n.io/t/oauth2-token-expires-every-7-days-for-google-drive-n8n-cloud-private-google-account-how-to-make-it-persistent/157586](https://community.n8n.io/t/oauth2-token-expires-every-7-days-for-google-drive-n8n-cloud-private-google-account-how-to-make-it-persistent/157586)  
24. Google Docs API | Google for Developers, accessed March 9, 2026, [https://developers.google.com/docs/api/reference/rest](https://developers.google.com/docs/api/reference/rest)  
25. TIL Google Docs can be exported to Markdown via URL change : r/LLMDevs \- Reddit, accessed March 9, 2026, [https://www.reddit.com/r/LLMDevs/comments/1r6q7tr/til\_google\_docs\_can\_be\_exported\_to\_markdown\_via/](https://www.reddit.com/r/LLMDevs/comments/1r6q7tr/til_google_docs_can_be_exported_to_markdown_via/)  
26. Convert Google Document to Markdown and vice versa using Google Apps Script · GitHub, accessed March 9, 2026, [https://gist.github.com/tanaikech/0deba74c2003d997f67fb2b04dedb1d0](https://gist.github.com/tanaikech/0deba74c2003d997f67fb2b04dedb1d0)  
27. Export MIME types for Google Workspace documents, accessed March 9, 2026, [https://developers.google.com/workspace/drive/api/guides/ref-export-formats](https://developers.google.com/workspace/drive/api/guides/ref-export-formats)  
28. Google Workspace and Google Drive supported MIME types, accessed March 9, 2026, [https://developers.google.com/workspace/drive/api/guides/mime-types](https://developers.google.com/workspace/drive/api/guides/mime-types)  
29. get file content of google docs using google drive API v3 \- Stack Overflow, accessed March 9, 2026, [https://stackoverflow.com/questions/39381563/get-file-content-of-google-docs-using-google-drive-api-v3](https://stackoverflow.com/questions/39381563/get-file-content-of-google-docs-using-google-drive-api-v3)  
30. REST Resource: files | Google Drive, accessed March 9, 2026, [https://developers.google.com/workspace/drive/api/reference/rest/v3/files](https://developers.google.com/workspace/drive/api/reference/rest/v3/files)  
31. Share your chats from Gemini Apps \- Computer \- Google Help, accessed March 9, 2026, [https://support.google.com/gemini/answer/13743730?hl=en\&co=GENIE.Platform%3DDesktop](https://support.google.com/gemini/answer/13743730?hl=en&co=GENIE.Platform%3DDesktop)  
32. Share your conversations | Gemini Enterprise \- Google Cloud Documentation, accessed March 9, 2026, [https://docs.cloud.google.com/gemini/enterprise/docs/share-conversations](https://docs.cloud.google.com/gemini/enterprise/docs/share-conversations)  
33. Interactions API | Gemini API \- Google AI for Developers, accessed March 9, 2026, [https://ai.google.dev/gemini-api/docs/interactions](https://ai.google.dev/gemini-api/docs/interactions)  
34. URL context | Gemini API | Google AI for Developers, accessed March 9, 2026, [https://ai.google.dev/gemini-api/docs/url-context](https://ai.google.dev/gemini-api/docs/url-context)  
35. Web Scraping with Gemini AI: Python Tutorial for Data Extraction \- Oxylabs, accessed March 9, 2026, [https://oxylabs.io/blog/gemini-web-scraping](https://oxylabs.io/blog/gemini-web-scraping)  
36. Web Scraping with Gemini AI in Python – Step-by-Step Guide \- Bright Data, accessed March 9, 2026, [https://brightdata.com/blog/web-data/web-scraping-with-gemini](https://brightdata.com/blog/web-data/web-scraping-with-gemini)  
37. Using OAuth 2.0 to Access Google APIs | Authorization | Google for ..., accessed March 9, 2026, [https://developers.google.com/identity/protocols/oauth2\#expiration](https://developers.google.com/identity/protocols/oauth2#expiration)  
38. OAuth2 refresh token expiration and Youtube API v3 \- Google Developer forums, accessed March 9, 2026, [https://discuss.google.dev/t/oauth2-refresh-token-expiration-and-youtube-api-v3/160874](https://discuss.google.dev/t/oauth2-refresh-token-expiration-and-youtube-api-v3/160874)  
39. Authorization Errors | Device Access \- Google for Developers, accessed March 9, 2026, [https://developers.google.com/nest/device-access/reference/errors/authorization](https://developers.google.com/nest/device-access/reference/errors/authorization)  
40. Gemini API Free Tier 2026: Complete Guide to Rate Limits, Models & Getting Started, accessed March 9, 2026, [https://blog.laozhang.ai/en/posts/gemini-api-free-tier](https://blog.laozhang.ai/en/posts/gemini-api-free-tier)

[image1]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACoAAAAYCAYAAACMcW/9AAAC0klEQVR4Xu2WWchNURTH/2YyJJkjZcoQMpSEFClDKZLhS77PkKIUJTI8kQdDSeJVpCQKmcqYIYSQkHlKiOJBhgfT/3/XPvess79z75MH5f7r1z177bX32Xvttde5QEUV1VIdMpIMIo2DrSNpUvT4BzSPnCHbyFHyjkwnj0lz59edLCcbSDXSDXmtIONIW9I6PMuWpxZkJllF+kR9taQXXyEtna0H+UVuONtUcpbUwMZ8Ii9JN+ejU/kd8Z3Mdz6JJpHnZA4ZS56SIRkPp77kJ+kXd1CXyObwrJ2/Jb3TbkyGLeSas0lfyUVyDhb5gdnuggaQj0g3OQo215aiRyRFRg7N4g5YGkwMzyNgfg/S7kLufgl2pUQiRamc6pNXZJ+zKb02In9TBW2HvWgdqRv1+UuliOrY5ed1Hza+q7PpCMtJ0dSYJbBU6ULqZTxypDxJcukDbJfKmUbeqYS0+B/kTmR/AVvEaVjfgkyvXVy9by05TnaS92SRd4qlHS0l35C9AKdgR1RO62G+syK75tJipFbkAjmCdL4dsHGPkAYkifK00C6ppmQCLJl1SzVIZaWU+sMWpByP1StqV8Pmqwnt/aG9JnEIUkm8HtkK0i7ytBA2kX7z1IE8Q37JkXRKXio9mm93aCf3oqroYXoS7G28sR2swOdpGGzA4LgDlpdXyRRn03Pn8Kx0UL75yzUaNt+B0F4W2n4OSR8X2dt7o3JBtzPvtm2FlZi4CjQgh8iYyH4S9gWS9pLPyB7/DNgCVoe2+tSOT+QNuYfoRJKEnu2N1HDYF0elyUuD95DXsDIlzsOKvSKYSHVXRT6Rxh2GfSx0sRKpIhxz7Z6w9Yx3toJuksWwHdwim2ALuY38S5TkWR6XnZ9OaBcs8orgCfIQtdNIx3uXHIR9/RTNlRmPoKHhtyHsH5Nqp3Lpb0mpoCPXFy3vj4ukaCsAc0mnqK+iiir67/QHoUKgnBFltYIAAAAASUVORK5CYII=>

[image2]: <data:image/png;base64,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>