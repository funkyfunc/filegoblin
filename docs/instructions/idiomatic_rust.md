# Idiomatic Rust for Autonomous Coding Agents

## 1. Memory Safety & The Ownership Model

Strict adherence to the borrow checker's static analysis is required to structurally eliminate memory defects. 

### Strict Boundaries for Borrowing (`&`) vs. Mutable Borrowing (`&mut`)

Idiomatic Rust minimizes the scope and duration of mutable borrows. The "Aliasing XOR Mutability" constraint is strictly enforced: multiple immutable borrows (`&T`) OR exactly one mutable borrow (`&mut T`).

- **Borrow Splitting:** Rather than locking an entire struct behind a method call taking `&mut self`, extract references to specific fields into local variables prior to mutation.

**Bad / Un-idiomatic:**
```rust
struct NetworkContext {
    packet_buffer: Vec<u8>,
    connection_metadata: String,
}

impl NetworkContext {
    // BAD: This method demands an exclusive lock on the entire struct,
    // paralyzing access to independent fields like `connection_metadata`.
    fn append_packet_data(&mut self) {
        self.packet_buffer.push(0xFF);
    }
}

fn execute_transaction(context: &mut NetworkContext) {
    let meta = &context.connection_metadata; // Immutable borrow established here
    context.append_packet_data();            // ERROR: Mutable borrow of `context` conflicts
    println!("Transaction on: {}", meta);
}
```

**Good / Idiomatic:**
```rust
fn execute_transaction(context: &mut NetworkContext) {
    // GOOD: Disjoint field borrowing. The compiler granularly tracks
    // borrows at the field level rather than the struct level.
    let meta = &context.connection_metadata;
    let buf = &mut context.packet_buffer;
    
    buf.push(0xFF);
    println!("Transaction on: {}", meta);
}
```

### Avoiding Unnecessary Allocation: Cow and the Cost of Cloning

- Avoid excessive use of `.clone()` or `.to_owned()`. Pass references wherever feasible.
- For operations that *might* require mutation but are predominantly read-only (e.g., string sanitization), utilize `std::borrow::Cow` (Clone-on-Write). It defers heap allocation until a mutation is explicitly required.

**Bad / Un-idiomatic:**
```rust
// BAD: Allocates a new String on the heap unconditionally every single time.
fn format_username_naive(name: &str) -> String {
    let mut clean_name = name.to_owned();
    if clean_name.starts_with('@') {
        clean_name.remove(0);
    }
    clean_name
}
```

**Good / Idiomatic:**
```rust
use std::borrow::Cow;

// GOOD: Avoids heap allocation entirely unless a mutation actually occurs.
fn format_username_optimized(name: &str) -> Cow<'_, str> {
    if name.starts_with('@') {
        Cow::Owned(name[1..].to_owned())
    } else {
        Cow::Borrowed(name)
    }
}
```

### Stack vs. Heap Allocation (`Box`, `Rc`, `Arc`)

Rust prefers stack allocation by default. When heap allocation is unavoidable, carefully select the appropriate smart pointer:

| Smart Pointer Type | Thread Safety | Mutability Paradigm | Idiomatic Architectural Use Case |
| :--- | :--- | :--- | :--- |
| **`Box<T>`** | Safe (`Send` / `Sync`) if `T` is safe | Mutable (`&mut`) via deref | The default choice for exclusive heap allocation. Guarantees singular ownership with zero runtime overhead. |
| **`Rc<T>`** | **Unsafe** (Not `Send` / `Sync`) | Immutable (`&`) | Utilized exclusively for single-threaded, shared ownership via deterministic reference counting. |
| **`Arc<T>`** | Safe (`Send` / `Sync`) if `T` is safe | Immutable (`&`) | Mandated for multi-threaded shared ownership. Utilizes hardware-level atomic CPU instructions. |

*Architectural Note:* To achieve shared *mutable* state, wrap the inner data type in an interior mutability primitive (e.g., `Rc<RefCell<T>>` for single-threaded, `Arc<Mutex<T>>` or `Arc<RwLock<T>>` for multi-threaded).

### Absolute Rule Regarding the `unsafe` Keyword

- **Directive:** The use of `unsafe` is strictly prohibited in standard, high-level application code.
- `unsafe` is permitted *only* under these strict criteria:
  1. **FFI Boundaries:** Interfacing directly with C-libraries or external OS APIs.
  2. **Hardware Abstraction Layers (HAL):** Bare-metal embedded systems or kernel driver development.
  3. **Provable Critical Paths:** When implementing foundational, heavily profiled data structures, accompanied by exhaustive fuzzing and runtime test coverage.

## 2. Idiomatic Flow Control & Functional Patterns

Idiomatic Rust heavily leverages zero-cost functional abstractions.

### Iterators and Bounds-Check Elimination

Use iterator chains (`.iter()`, `.map()`, `.filter()`, `.fold()`) instead of raw indexing (`array[i]`) to allow the compiler to eliminate runtime bounds checks and maximize execution speed safely.

**Bad / Un-idiomatic:**
```rust
// BAD: Imperative loop forcing a runtime bounds check on every single iteration.
fn sum_even_imperative(numbers: &[i32]) -> i32 {
    let mut sum = 0;
    for i in 0..numbers.len() {
        if numbers[i] % 2 == 0 {
            sum += numbers[i];
        }
    }
    sum
}
```

**Good / Idiomatic:**
```rust
// GOOD: Zero-cost functional abstraction. Iterators eliminate bounds checks entirely.
fn sum_even_functional(numbers: &[i32]) -> i32 {
    numbers.iter()
          .filter(|&x| x % 2 == 0)
          .sum()
}
```

### Pattern Matching vs. Deep Nesting

Prioritize `match`, `if let`, or `while let` control structures over imperative `is_some()` or `is_ok()` validation checks to cleanly handle state.

**Bad / Un-idiomatic:**
```rust
// BAD: Deeply nested and relies on manual unwrapping.
fn process_optional_data(data: Option<String>) {
    if data.is_some() {
        let value = data.unwrap();
        if value.starts_with("SYS_") {
            println!("Valid System Metric: {}", value);
        }
    }
}
```

**Good / Idiomatic:**
```rust
// GOOD: Flattens logical nesting and binds variables safely within the pattern itself.
fn process_optional_data(data: Option<String>) {
    if let Some(value) = data {
        if value.starts_with("SYS_") {
            println!("Valid System Metric: {}", value);
        }
    }
}
```

### Standard Traits for Type Conversion and Instantiation

- Standardize type conversion through `From`, `Into`, `TryFrom`, and `TryInto`. Never author ad-hoc `.to_string()`, `.as_custom_type()`, or `.convert()` methods.
- Implement the `std::default::Default` trait for struct initialization rather than exclusively exposing an empty, parameterless `new()` method.

## 3. Robust Error Handling

Error handling in Rust is strictly explicit via the `Result<T, E>` and `Option<T>` enumerations.

### Banning Anti-Patterns: Panics in Production

- The indiscriminate use of `.unwrap()` and `.expect()` is strictly prohibited in production application logic. Treat every unwrap as a critical failure point.

**Bad / Un-idiomatic:**
```rust
// BAD: Hard crashes the entire application on read failure.
fn read_system_config(path: &str) -> String {
    std::fs::read_to_string(path).unwrap() 
}
```

**Good / Idiomatic:**
```rust
// GOOD: Safely propagates the error to the caller.
fn read_system_config(path: &str) -> Result<String, std::io::Error> {
    std::fs::read_to_string(path)
}
```

### Idiomatic Error Propagation: The `?` Operator

Propagate errors across system boundaries using the `?` operator. It automatically unwraps `Ok`/`Some` values or triggers an early return for `Err` values, implicitly invoking the `From::from()` trait method for seamless error casting.

### Industry-Standard Error Architectures: `thiserror` vs. `anyhow`

The choice of crate must be determined strictly by the architectural boundary:

1. **Library Crates (`thiserror`):** Use `thiserror` to define explicit, custom enum error types when authoring a library intended to be consumed by other developers or distinct microservices.
2. **Application Binaries (`anyhow`):** Use `anyhow` when authoring top-level application logic (compiled binary, CLI, API routing) to aggregate and log opaque errors with `.context()`.

| Feature Comparison | `thiserror` Crates | `anyhow` Crates |
| :--- | :--- | :--- |
| **Primary Domain** | Reusable Library Crates | Executable Applications / Binaries |
| **Error Type** | Custom, concrete enum trees | Opaque, unified `anyhow::Error` |
| **Consumer Action** | Programmatic matching and specific recovery | Contextual logging and graceful termination |
| **Performance Profile** | Zero-overhead, static dispatch | Minor runtime overhead (boxing/trait objects) |

## 4. Type System & Polymorphism (Traits over Inheritance)

Achieve polymorphism, abstraction, and code reuse exclusively through Composition and Traits.

### Composition and Enums over Subclassing

- Build complex entities by composing smaller, single-purpose structs.
- For closed sets of behavioral variations (e.g., states in an FSM, event types), employ algebraic data types via the `enum` keyword.

### Precise Control of Dispatch: Static vs. Dynamic

- **Static Dispatch (Generics and Trait Bounds: `<T: Trait>`):** By default, architect polymorphic functions utilizing Generics or `impl Trait`. This utilizes monomorphization for absolute zero runtime overhead and maximum execution speed.
- **Dynamic Dispatch (Trait Objects: `Box<dyn Trait>`):** Utilize dynamic dispatch *only* when architectural constraints demand runtime flexibility (e.g., storing heterogeneous, varying types implementing the same trait in a single collection).

**Bad / Un-idiomatic (Unnecessary Dynamic Dispatch):**
```rust
trait EventProcessor {
    fn process(&self);
}

// BAD: Incurs severe runtime overhead via vtable lookups and fat pointers.
fn run_processor(p: Box<dyn EventProcessor>) {
    p.process();
}
```

**Good / Idiomatic (Static Dispatch via Monomorphization):**
```rust
// GOOD: Zero-cost abstraction. 
fn run_processor<T: EventProcessor>(p: &T) {
    p.process();
}
```

## 5. Automated Enforcement & Tooling

Code generated by the agent must pass the language's native validation commands.

### Mandatory Validation Pipeline

1. `cargo fmt --check`: Enforces standard formatting.
2. `cargo test`: Executes unit and integration tests.
3. `cargo clippy`: Identifies un-idiomatic patterns.

### Strict Compliance with `cargo clippy`

Execute Clippy across all targets and features, elevating warnings to fatal compiler errors:
```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Assume the following strict lint configurations exist in `Cargo.toml`:

| Lint Category / Rule | State | Architectural Rationale for Restriction |
| :--- | :--- | :--- |
| `clippy::all` | "deny" | Elevates all standard lints to deny to prevent gradual technical debt accumulation. |
| `clippy::correctness` | "deny" | Code that is outright wrong, logically flawed, or useless. |
| `clippy::complexity` | "deny" | Code that does something simple in an overly complex or convoluted manner. |
| `clippy::perf` | "deny" | Code that forces unnecessary allocations or inhibits LLVM optimization. |
| `unwrap_used` | "deny" | Forbids `.unwrap()` in favor of explicit error propagation. |
| `expect_used` | "deny" | Forbids `.expect()`, enforcing identical fallibility paths as unwrap. |
| `panic` | "deny" | Forbids the explicit `panic!` macro, preventing engineered thread crashes. |
| `indexing_slicing` | "deny" | Forbids unsafe raw index accesses (e.g., `array[i]`) in favor of zero-cost `.iter()` or safe `.get(i)`. |
| `unreachable` | "deny" | Flags and rejects dead code paths to ensure comprehensive logical flow. |
| `undocumented_unsafe_blocks` | "deny" | Mandates explicitly documented `# Safety` justifications above all unsafe regions. |
