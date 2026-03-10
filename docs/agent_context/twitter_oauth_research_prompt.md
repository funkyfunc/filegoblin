# **Technical architecture and economic governance of the X Platform API v2: A 2026 strategic assessment**

The transition of the X Platform (formerly Twitter) from a developer-centric social ecosystem to a tightly monetized, enterprise-grade data utility has redefined the requirements for programmatic interaction. For modern implementations, particularly those transitioning from legacy "guest-token" methodologies to stable, authenticated workflows, the platform now mandates a sophisticated understanding of OAuth 2.0 Proof Key for Code Exchange (PKCE) and a highly stratified pricing architecture.1 Since the acquisition in late 2022 and the subsequent rebranding in 2024, the technical barriers to entry have increased significantly, prioritizing professional integrations over hobbyist or unauthenticated scraping.1 This report provides an exhaustive technical analysis of the 2026 X API environment, covering authentication protocols, permission scopes, tiered access constraints, and the newly operationalized Pay-Per-Use (PPU) consumption model.

## **Evolution of the authentication subsystem: OAuth 2.0 PKCE**

The pivot from OAuth 1.0a to OAuth 2.0 represents the most significant technical modernization of the platform’s interface. While OAuth 1.0a remains available for legacy applications, the v2 API endpoints are designed to interact primarily with OAuth 2.0 tokens.1 For public clients—specifically Command Line Interface (CLI) tools, mobile applications, and single-page web apps where a client secret cannot be securely stored—the PKCE extension is the mandatory standard.3

### **Cryptographic mechanics of PKCE in the X environment**

The PKCE flow mitigates the risk of authorization code interception by requiring the client to prove its identity through a cryptographic challenge.3 This process involves the generation of a high-entropy code\_verifier, typically a random string between 43 and 128 characters in length.4 The relationship between the verifier and the challenge is defined by the ![][image1] transformation, which the platform requires for all modern integrations:

![][image2]  
During the initial authorization request, the application sends the code\_challenge and the code\_challenge\_method to the authorization server. The server stores this challenge and associates it with the issued authorization code.3 Upon the subsequent token exchange, the application must provide the original code\_verifier. The server then hashes this verifier and confirms it matches the previously stored challenge before issuing an access\_token.3

### **Canonical authentication and token endpoints**

As of early 2026, the X API continues to utilize the twitter.com domain for authentication endpoints to ensure compatibility with existing SDKs and library configurations, while API data requests are routed through the api.twitter.com or api.x.com subdomains.5

| Endpoint Function | Current (2026) URL | HTTP Method |
| :---- | :---- | :---- |
| **Authorization URL** | https://twitter.com/i/oauth2/authorize | GET (Redirect) |
| **Token Exchange URL** | https://api.twitter.com/2/oauth2/token | POST |
| **Token Refresh URL** | https://api.twitter.com/2/oauth2/token | POST |
| **User Identity (me)** | https://api.twitter.com/2/users/me | GET |

The persistence of the twitter.com domain for authorization is a strategic decision by the platform to minimize friction for existing integrations during the rebranding process.1 Developers must ensure that their HTTP clients, such as those implemented in Rust or Python, correctly handle the redirects and maintain the state of the PKCE exchange across these distinct endpoints.3

## **Permission architectures: Granular scopes and user consent**

The v2 API utilizes a granular scope system that allows users to grant specific permissions to applications rather than a broad read/write binary.7 For a thread-ingestion tool designed to read conversations and identify users, the following scopes are foundational for operation.

### **Mandatory scope strings for content ingestion**

To effectively retrieve a single tweet, reconstruct a full conversation thread, and resolve author profiles, the application must request a specific subset of permissions.6

| Scope String | Permission Description | Requirement Level |
| :---- | :---- | :---- |
| tweet.read | Allows reading of public tweets, including those in a conversation thread. | Mandatory |
| users.read | Grants access to basic profile fields (username, ID, display name). | Mandatory |
| offline.access | Enables the issuance of a refresh\_token for persistent sessions. | Mandatory for CLI |
| like.read | (Optional) Allows the tool to see which posts a user has liked. | Contextual |
| bookmark.read | (Optional) Allows the retrieval of a user's private bookmarks. | Contextual |

The offline.access scope remains the correct and necessary string to obtain a long-lived refresh\_token.4 Without this scope, the authorization server will only return an access\_token with a limited lifespan, necessitating frequent re-authentication through the browser.10 For CLI tools that aim for a "one-time login" user experience, the exclusion of offline.access is a common failure point in the authentication state machine.

### **Verification of identity via the Users endpoint**

Once an application is authorized, the first operation should be a call to the GET /2/users/me endpoint. This call confirms that the token is active and allows the application to store the user's account details locally.5 This endpoint returns the id, name, and username by default, which are critical for anchoring the local credentials.json file to a specific X account.

## **Economic governance: The 2026 tier system**

The most profound change in the X developer ecosystem is the transition from a data-rich free tier to a "write-only" gate followed by high-cost entry points.2 For any application requiring the ability to read tweet threads, the "Free" tier is no longer a viable option.11

### **Stratified access tiers and read/write quotas**

The following table outlines the monthly costs and functional limits for the primary access levels as of 2026\.

| Tier Name | Monthly Cost | Monthly Read Limit | Monthly Write Limit | Search Access |
| :---- | :---- | :---- | :---- | :---- |
| **Free** | $0 | \~1 req / 24 hours | 500 posts | No Search |
| **Basic** | $200 | 15,000 posts | 50,000 posts | Recent (7 days) |
| **Pro** | $5,000 | 1,000,000 posts | 300,000 posts | Full Archive |
| **Enterprise** | $42,000+ | Custom | Custom | Custom / Streaming |

The "Free" tier is strictly designed for write-only use cases, such as simple automated bots that post updates.11 While a developer can create a free app and use OAuth 2.0 PKCE, any request to read a tweet ID or search for a thread will trigger rate limits that effectively block the application from functioning.1 For a CLI tool to fetch threads, the "Basic" tier is the true entry point, representing a $200 per month commitment that may be prohibitive for individual developers or small open-source projects.1

### **The Pay-Per-Use (PPU) consumption model**

In February 2026, X officially launched a consumption-based pricing model to address the "missing middle" between the $200 and $5,000 price points.11 This model allows developers to buy credits and pay only for the resources they fetch.1

Under the PPU model, the cost per operation is as follows:

* **Post Read:** $0.005 per resource.1  
* **User Profile Read:** $0.010 per resource.1  
* **Content Creation:** $0.010 per request.1

For a CLI tool, this is the most viable official path. It allows a user to fetch a 20-tweet thread for approximately $0.11 (including the user profile lookup).1 X provides a one-time $10 voucher to legacy free-tier users who move to the PPU model to facilitate this transition.11 This credit-based system includes auto-top-up features and spending caps to prevent unexpected financial exposure.12

## **App registration and callback constraints**

Registration of an application on the X Developer Portal is generally instant, taking approximately 5 to 10 minutes for basic profile approval.1 However, the configuration of the OAuth 2.0 settings requires precise attention to the redirection logic.

### **Localhost support for CLI applications**

X officially supports http://localhost and 127.0.0.1 as callback URIs for PKCE implementations.4 Because the traffic to a loopback address never leaves the local machine, the platform does not require the use of HTTPS for these specific redirects, which simplifies the development of CLI tools.14

Requirements for the callback URI:

1. The URI must be explicitly whitelisted in the Developer Portal.4  
2. If the application uses a specific port (e.g., http://localhost:7890/callback), that exact port must be part of the whitelisted string.4  
3. The use of 127.0.0.1 and localhost are treated as distinct; developers should whitelist both to ensure cross-platform compatibility.13

A common architecture for a Rust CLI tool involves starting a temporary local web server using a library like tiny\_http or warp to listen for the incoming authorization code once the user completes the browser-based login.6

## **Token lifecycle and persistence strategies**

Managing the lifetime of tokens is critical for maintaining a stable implementation. Unlike the brittle "guest tokens" which rotate frequently and without warning, OAuth 2.0 tokens follow a predictable lifecycle defined by the RFC 6749 standard.17

### **Access Token Time-to-Live (TTL)**

The access\_token issued by X typically has a lifespan of 7,200 seconds, or 2 hours.10 After this period, the token is revoked and any API calls made with it will return a 401 Unauthorized error.10 The application must use the expires\_in field returned during the initial token exchange to track this expiration locally and proactively refresh the token.

### **Refresh token rotation and re-authentication**

X implements a rotating refresh token policy.21 When an application uses a refresh\_token to obtain a new access\_token, the authorization server also returns a *new* refresh\_token.10 The previous refresh\_token is invalidated immediately.10

For a tool like filegoblin, the storage strategy in \~/.config/filegoblin/credentials.json must account for this rotation:

* The application must overwrite the old refresh\_token with the new one every time a refresh occurs.  
* The refresh\_token itself is long-lived (often 30 to 90 days), but it will expire if not used within its valid window.21  
* If a refresh\_token is allowed to expire or is revoked by the user, the application must discard the local credentials and force the user to re-authenticate through the full PKCE browser flow.10

## **Technical mechanics of thread retrieval**

The reconstruction of a conversation thread is one of the more complex operations in the v2 API, as there is no single "get\_thread" endpoint. Instead, the developer must leverage the conversation\_id field.24

### **The conversation lookup workflow**

A conversation thread is defined as a series of tweets that share a common conversation\_id, which is the tweet ID of the very first post in the sequence.24 To fetch a thread, the following logic is required:

1. **Seed Tweet Lookup:** Call GET /2/tweets/:id for the starting tweet. The response will include the conversation\_id in the metadata if requested.5  
2. **Thread Search:** Use the GET /2/tweets/search/recent endpoint with a query parameter such as query=conversation\_id:123456789.5  
3. **Author Filtering:** To ensure only the thread author’s posts are retrieved (excluding replies from other users), the query should be refined: query=conversation\_id:123456789 from:author\_username.25

### **Tier-based search constraints**

The availability of these search endpoints is the primary differentiator between the access tiers.

| Feature | Free Tier | Basic Tier | Pro Tier |
| :---- | :---- | :---- | :---- |
| **Tweet Lookup (by ID)** | Very Limited | 15,000 / mo | 1,000,000 / mo |
| **Recent Search (7 days)** | No Access | 60 req / 15 min | 300 req / 15 min |
| **Full Archive Search** | No Access | No Access | 300 req / 15 min |

The technical implication is clear: **thread fetching requires at least the Basic tier or the PPU model**.1 A tool implemented against the Free tier will fail at the second step of the thread reconstruction process because it cannot access the search endpoint required to find replies associated with a conversation\_id.2 Furthermore, threads older than 7 days can only be retrieved by users on the Pro tier or the Enterprise tier, as the Basic tier search index is limited to the preceding week.2

## **Post-rebrand breaking changes and implementation pitfalls**

Developers building against 2022-era documentation will encounter several breaking changes that reflect the platform's shift in philosophy and branding.

### **Branding and domain shifts**

While the API technically answers to legacy twitter.com URIs, the branding is now exclusively X. All user-facing authorization screens and email notifications will refer to "X".1 Developers should update their application descriptions and UI to reflect this rebranding to avoid confusing users during the OAuth consent flow.

### **Discontinuation of guest-token accessibility**

The unauthenticated "guest-token hack" has become increasingly brittle.17 X has implemented more aggressive rotation of these tokens and has introduced sophisticated IP-based rate limiting designed specifically to break unauthenticated scraping.1 Modern tools must treat guest tokens as a legacy fallback with a high probability of failure. The official OAuth 2.0 PKCE flow is the only supported path for stable, long-term access.1

### **Deduplication and cost optimization in PPU**

The 2026 PPU model introduces a "soft guarantee" of deduplication.12 If an application requests the same post multiple times within a single 24-hour UTC window, it is only charged once.12 However, this window resets at midnight UTC.12

Implementation considerations for cost optimization:

* **Local Caching:** Applications should store retrieved tweets in a local database or Markdown file to avoid re-fetching and incurring costs on subsequent days.1  
* **Field Selection:** The API allows developers to specify which fields they want in the response. requesting only the fields needed (e.g., text, author\_id, conversation\_id) reduces response size and potentially impacts the computational load, though billing is currently per-resource rather than per-byte.1  
* **Batching:** When looking up multiple tweets by ID, developers should use the batch endpoint GET /2/tweets?ids=... to maximize the value of each request, as looking up 100 tweets in one call is more efficient than 100 individual calls.1

## **Strategic summary of implementation requirements**

The following table synthesizes the essential deliverables for a 2026 X API integration focused on thread ingestion.

| Requirement | 2026 Live Value / Status |
| :---- | :---- |
| **Auth URL** | https://twitter.com/i/oauth2/authorize |
| **Token URL** | https://api.twitter.com/2/oauth2/token |
| **Scope String** | tweet.read users.read offline.access |
| **Free Tier Viability** | **No** (Write-only, no search access) |
| **Basic Tier Cost** | $200 / month |
| **PPU Cost** | \~$0.005 per tweet read |
| **Localhost Support** | Fully supported for PKCE callbacks |
| **Token TTL** | 2 Hours (Access Token) |
| **Refresh Strategy** | Rotating tokens; must store new refresh\_token on every use |
| **Thread Fetching** | Requires conversation\_id filter via Recent Search |

The transition to an authenticated model represents a professionalization of the tool's interface. By implementing the OAuth 2.0 PKCE flow, the developer gains access to a stable, supported platform that is less susceptible to silent breaking changes. While the economic cost is now the primary barrier, the introduction of the Pay-Per-Use model in early 2026 provides a scalable entry point that was previously absent, allowing for high-quality data ingestion without the burden of an enterprise-level subscription. Successful implementation in Rust will require careful management of the state machine, secure credential storage, and a robust error-handling logic that distinguishes between transient rate limits and permanent authorization failures.

#### **Works cited**

1. How to Get X API Key: Complete 2026 Guide to Pricing, Setup & Optimization \- Elfsight, accessed March 9, 2026, [https://elfsight.com/blog/how-to-get-x-twitter-api-key-in-2026/](https://elfsight.com/blog/how-to-get-x-twitter-api-key-in-2026/)  
2. Twitter API Pricing 2026: Tiers, Costs & Alternatives \- Xpoz, accessed March 9, 2026, [https://www.xpoz.ai/blog/guides/understanding-twitter-api-pricing-tiers-and-alternatives/](https://www.xpoz.ai/blog/guides/understanding-twitter-api-pricing-tiers-and-alternatives/)  
3. How to Connect Users' X (Twitter) Accounts Using OAuth 2.0 \+ PKCE \- Medium, accessed March 9, 2026, [https://medium.com/@aayushman2702/how-to-connect-users-x-twitter-accounts-using-oauth-2-0-pkce-d98c091b2bb4](https://medium.com/@aayushman2702/how-to-connect-users-x-twitter-accounts-using-oauth-2-0-pkce-d98c091b2bb4)  
4. Auth Code Flow With PKCE | TradeStation API, accessed March 9, 2026, [https://api.tradestation.com/docs/fundamentals/authentication/auth-pkce/](https://api.tradestation.com/docs/fundamentals/authentication/auth-pkce/)  
5. X OAuth endpoints \- Logto, accessed March 9, 2026, [https://logto.io/oauth-providers-explorer/x](https://logto.io/oauth-providers-explorer/x)  
6. Day 47 of 100 Days Agentic Engineer Challenge: X API with Sveltekit and FastAPI, accessed March 9, 2026, [https://damiandabrowski.medium.com/day-47-of-100-days-agentic-engineer-challenge-x-api-with-sveltekit-and-fastapi-3842513c8ada](https://damiandabrowski.medium.com/day-47-of-100-days-agentic-engineer-challenge-x-api-with-sveltekit-and-fastapi-3842513c8ada)  
7. X/Twitter OAuth 2.0 fails with 400 \- Supabase sending invalid users.email scope \- Answer Overflow, accessed March 9, 2026, [https://www.answeroverflow.com/m/1459601433900093515](https://www.answeroverflow.com/m/1459601433900093515)  
8. smolblog/oauth2-twitter \- Packagist.org, accessed March 9, 2026, [https://packagist.org/packages/smolblog/oauth2-twitter](https://packagist.org/packages/smolblog/oauth2-twitter)  
9. How to fetch a user's bookmarks with the X API as of February 2026 \- GitHub Gist, accessed March 9, 2026, [https://gist.github.com/peterc/7f3d55d46c02f662e5a5e08e070954be](https://gist.github.com/peterc/7f3d55d46c02f662e5a5e08e070954be)  
10. Offline access tokens now support expiry and refresh \- Shopify developer changelog, accessed March 9, 2026, [https://shopify.dev/changelog/offline-access-tokens-now-support-expiry-and-refresh](https://shopify.dev/changelog/offline-access-tokens-now-support-expiry-and-refresh)  
11. X API Pricing in 2026: Every Tier Explained (And the New Pay-As ..., accessed March 9, 2026, [https://www.wearefounders.uk/the-x-api-price-hike-a-blow-to-indie-hackers/](https://www.wearefounders.uk/the-x-api-price-hike-a-blow-to-indie-hackers/)  
12. X (formerly Twitter) announces new pay-as-you-go pricing model for ..., accessed March 9, 2026, [https://gigazine.net/gsc\_news/en/20260209-x-api-pay-per-use/](https://gigazine.net/gsc_news/en/20260209-x-api-pay-per-use/)  
13. Twitter oAuth callbackUrl \- localhost development \- Stack Overflow, accessed March 9, 2026, [https://stackoverflow.com/questions/800827/twitter-oauth-callbackurl-localhost-development](https://stackoverflow.com/questions/800827/twitter-oauth-callbackurl-localhost-development)  
14. 3 easy ways to do OAuth redirects on localhost (with HTTPS) | Nango Blog, accessed March 9, 2026, [https://nango.dev/blog/oauth-redirects-on-localhost-with-https](https://nango.dev/blog/oauth-redirects-on-localhost-with-https)  
15. Twitter Log in with Localhost \- Stack Overflow, accessed March 9, 2026, [https://stackoverflow.com/questions/60495981/twitter-log-in-with-localhost](https://stackoverflow.com/questions/60495981/twitter-log-in-with-localhost)  
16. What redirect URI should I use for an authorization call used in an Electron app?, accessed March 9, 2026, [https://stackoverflow.com/questions/64530295/what-redirect-uri-should-i-use-for-an-authorization-call-used-in-an-electron-app](https://stackoverflow.com/questions/64530295/what-redirect-uri-should-i-use-for-an-authorization-call-used-in-an-electron-app)  
17. How to connect to endpoints using OAuth 2.0 Authorization Code ..., accessed March 9, 2026, [https://developer.x.com/en/docs/authentication/oauth-2-0/user-access-token](https://developer.x.com/en/docs/authentication/oauth-2-0/user-access-token)  
18. Get user\_access\_token \- Server API \- Documentation \- Feishu Open Platform, accessed March 9, 2026, [https://open.feishu.cn/document/uAjLw4CM/ukTMukTMukTM/authentication-management/access-token/get-user-access-token](https://open.feishu.cn/document/uAjLw4CM/ukTMukTMukTM/authentication-management/access-token/get-user-access-token)  
19. Token Endpoint \- APIs at athenahealth | API Solutions, accessed March 9, 2026, [https://docs.athenahealth.com/api/guides/token-endpoint](https://docs.athenahealth.com/api/guides/token-endpoint)  
20. OAuth for user authorized apps \- Rooms \- Zoom Developer Docs, accessed March 9, 2026, [https://developers.zoom.us/docs/rooms/oauth/](https://developers.zoom.us/docs/rooms/oauth/)  
21. Announcing the required expiration of global OAuth access and refresh tokens, accessed March 9, 2026, [https://support.zendesk.com/hc/en-us/articles/10212201463962-Announcing-the-required-expiration-of-global-OAuth-access-and-refresh-tokens](https://support.zendesk.com/hc/en-us/articles/10212201463962-Announcing-the-required-expiration-of-global-OAuth-access-and-refresh-tokens)  
22. Refresh Tokens \- Auth0 Docs, accessed March 9, 2026, [https://auth0.com/docs/secure/tokens/refresh-tokens](https://auth0.com/docs/secure/tokens/refresh-tokens)  
23. Access Token vs Refresh Token: A Breakdown \- Descope, accessed March 9, 2026, [https://www.descope.com/blog/post/access-token-vs-refresh-token](https://www.descope.com/blog/post/access-token-vs-refresh-token)  
24. Notes on Downloading Conversations through Twitter's V2 API \- Conrad Borchers, accessed March 9, 2026, [https://cborchers.com/2021/03/23/notes-on-downloading-conversations-through-twitters-v2-api/](https://cborchers.com/2021/03/23/notes-on-downloading-conversations-through-twitters-v2-api/)  
25. Get Threads Twitter API V2 \- Stack Overflow, accessed March 9, 2026, [https://stackoverflow.com/questions/73520237/get-threads-twitter-api-v2](https://stackoverflow.com/questions/73520237/get-threads-twitter-api-v2)

[image1]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACoAAAAYCAYAAACMcW/9AAAC0klEQVR4Xu2WWchNURTH/2YyJJkjZcoQMpSEFClDKZLhS77PkKIUJTI8kQdDSeJVpCQKmcqYIYSQkHlKiOJBhgfT/3/XPvess79z75MH5f7r1z177bX32Xvttde5QEUV1VIdMpIMIo2DrSNpUvT4BzSPnCHbyFHyjkwnj0lz59edLCcbSDXSDXmtIONIW9I6PMuWpxZkJllF+kR9taQXXyEtna0H+UVuONtUcpbUwMZ8Ii9JN+ejU/kd8Z3Mdz6JJpHnZA4ZS56SIRkPp77kJ+kXd1CXyObwrJ2/Jb3TbkyGLeSas0lfyUVyDhb5gdnuggaQj0g3OQo215aiRyRFRg7N4g5YGkwMzyNgfg/S7kLufgl2pUQiRamc6pNXZJ+zKb02In9TBW2HvWgdqRv1+UuliOrY5ed1Hza+q7PpCMtJ0dSYJbBU6ULqZTxypDxJcukDbJfKmUbeqYS0+B/kTmR/AVvEaVjfgkyvXVy9by05TnaS92SRd4qlHS0l35C9AKdgR1RO62G+syK75tJipFbkAjmCdL4dsHGPkAYkifK00C6ppmQCLJl1SzVIZaWU+sMWpByP1StqV8Pmqwnt/aG9JnEIUkm8HtkK0i7ytBA2kX7z1IE8Q37JkXRKXio9mm93aCf3oqroYXoS7G28sR2swOdpGGzA4LgDlpdXyRRn03Pn8Kx0UL75yzUaNt+B0F4W2n4OSR8X2dt7o3JBtzPvtm2FlZi4CjQgh8iYyH4S9gWS9pLPyB7/DNgCVoe2+tSOT+QNuYfoRJKEnu2N1HDYF0elyUuD95DXsDIlzsOKvSKYSHVXRT6Rxh2GfSx0sRKpIhxz7Z6w9Yx3toJuksWwHdwim2ALuY38S5TkWR6XnZ9OaBcs8orgCfIQtdNIx3uXHIR9/RTNlRmPoKHhtyHsH5Nqp3Lpb0mpoCPXFy3vj4ukaCsAc0mnqK+iiir67/QHoUKgnBFltYIAAAAASUVORK5CYII=>

[image2]: <data:image/png;base64,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>