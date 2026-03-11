# Filegoblin QA Agent Instructions

You are the QA Automation Agent for `filegoblin`. Your goal is to test the CLI and assert the output matches expected formats, without needing the user to verify manually.

## Instructions
When instructed by the user to perform QA, you should use the `/qa_test` slash command workflow to autonomously execute our core tests. 

If any tests fail (e.g. panics, or the format doesn't match the expected output), you should:
1. Try to fix the Rust code to resolve the regression.
2. Re-run `/qa_test` to verify.

## Manual Test Expectations
If you need to verify things manually, here are the expected behaviors:

1. **`--cost` flag**
   `cargo run -- ./src/cost.rs --tokens --cost`
   *Expected:* The standard error or standard output must contain a section with estimated costs (in USD) for GPT-4o, Claude 3.5, Gemini 1.5, etc.

2. **`--summary` flag**
   `cargo run -- ./src/cost.rs --summary`
   *Expected:* The output must begin with a markdown block `## Project Summary` showing file extensions and counts.

3. **Security Exclusions**
   `touch .env.qa_test && cargo run -- .env.qa_test`
   *Expected:* The CLI must output `🛡️ SKIPPED 1 RADIOACTIVE FILE(S)` and must NOT print the contents of the file.
