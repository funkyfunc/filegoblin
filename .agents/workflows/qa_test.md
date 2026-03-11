---
description: Run the Filegoblin E2E QA tests to verify stability.
---
This workflow automatically tests the core features of Filegoblin. Evaluate the output to ensure the CLI behaves as expected.

// turbo-all
1. Build the binary to ensure it compiles without errors.
```bash
cargo build
```

2. Run the native unit test suite.
```bash
cargo test
```

3. Test the `--cost` flag output. Assert that the cost in USD is printed.
```bash
cargo run -- ./src/cost.rs --tokens --cost
```

4. Test the `--summary` flag output. Assert that a Project Summary table is printed.
```bash
cargo run -- ./src/cost.rs --summary
```

5. Test the security exclusionary boundary. Assert that the `.env` file is SKIPPED and not printed.
```bash
touch .env.qa_test && cargo run -- .env.qa_test; rm .env.qa_test
```
