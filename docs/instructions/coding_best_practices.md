# Coding Best Practices

You are an autonomous coding agent. These are your standing instructions for writing, modifying, and refactoring code. They are language-agnostic. Apply them on every task unless a language-specific instruction file overrides a specific point.

Every item is a **heuristic with tradeoffs**, not a rigid law. Each carries a **"Bend when"** note — use it. But when you have no strong reason to deviate, follow the heuristic.

## Conflict Resolution

When principles below conflict, resolve using this precedence:

1. Correctness and preserving existing behavior
2. Security — never introduce regressions (see Security Baseline)
3. Readability and local reasoning
4. Consistency with the existing codebase
5. Simplicity / fewer moving parts
6. Everything else (DRY, abstraction, optimization)

---

## Core Principles

### Readability Over Cleverness
Optimize for the next reader, not the fewest characters.
- Write explicit logic, descriptive names, obvious control flow.
- Do not write dense one-liners, obscure idioms, or trick syntax that hides intent.
- **Bend when:** a well-understood idiom already common in the codebase is genuinely clearer than the longhand.

### Simplicity (KISS)
Build the simplest thing that solves the actual problem.
- Use standard constructs, flat control flow, fewer concepts.
- Do not add layers, indirection, or configurability the task does not require.
- **Bend when:** the problem is inherently complex (correctness, concurrency, a real constraint). Isolate that complexity behind a clearly named boundary.

### YAGNI
Build for today's requirements. Do not pre-build for imagined futures.
- Implement exactly what is asked. Do not add speculative parameters, hooks, or "just in case" generality.
- **Bend when:** a near-certain, imminent need exists and retrofitting later would be costly. Add the minimal seam, not the full feature.

### DRY, Applied Carefully
Remove duplication of **knowledge**, not duplication of **text**. Duplication is cheaper than the wrong abstraction.
- Unify when a single requirement change would force identical edits in both places.
- Do not merge code that only looks alike but changes for different reasons — that creates hidden coupling.
- **Bend when:** uncertain — prefer duplication. Wait for the shared concept to prove itself (see Abstraction Guidance).

### Single Responsibility
A unit should have one reason to change.
- **Test:** can you state its job in one sentence without "and"? If a function name requires "And" (e.g., `validateAndSave`), split it.
- **Bend when:** closely related actions read more clearly together than scattered across many tiny units.

### Clear Naming
Names are the primary documentation.
- Use names that state role/meaning in the **domain's vocabulary**. Favor descriptive names (`active_user_count`, not `n`).
- Do not use vague names (`data`, `tmp`, `doProcess`), misleading names, or cryptic abbreviations.
- Match name length to scope. A 3-line loop index can be `i`.

### Explicitness and Least Surprise
Make effects, inputs, and requirements visible.
- Pass dependencies in. Name side-effecting functions for what they change. Surface ordering and requirements in the signature.
- Favor explicit declarations over metaprogramming (dynamic code/property generation via reflection), which destroys static analysis.
- Do not rely on hidden global reads, action-at-a-distance, or surprising overloads.
- **Bend when:** the metaprogramming is language-idiomatic (e.g., Python decorators, Rust derive macros) and well-understood.

### Consistency with the Existing Codebase
Match the conventions already in the repo over your own preference.
- Use existing patterns, libraries, naming, and structure for similar tasks.
- Do not introduce a second way to do something already solved in this codebase.
- **Bend when:** the existing pattern is demonstrably harmful — then change it deliberately and consistently, as its own change.

### Evidence-Based Optimization
Make it correct and clear first. Optimize only against measurement or a stated constraint.
- Do not speculate on performance. Do not trade readability for unverified speed.
- **When you must optimize:** localize it, comment the reason and measured gain, keep the clear version recoverable.

### Safe Incremental Change
Prefer many small, verifiable changes over one large one.
- Keep the build green at each step. Make commits separable and individually reviewable.
- Do not bundle unrelated edits or attempt big-bang rewrites.
- **Bend when:** an atomic cross-cutting change is unavoidable — sequence it (add new → migrate → remove old).

---

## Abstraction Guidance

Default to explicit, concrete code. Let abstractions earn their place.

**Decision order:**
1. One occurrence, or only coincidental similarity → **keep it explicit.**
2. The same *knowledge* proven in ~3 places, changing together → **extract a helper** (rule of three).
3. A stable, nameable concern with its own vocabulary → **create a module boundary.**
4. Uncertain future need → **defer.**

**Signs of a bad abstraction — inline it:**
- A helper grows boolean flags or mode params to serve different callers.
- "Util/manager/helper" grab-bags collecting unrelated functions.
- You must trace many thin wrappers to understand one operation.
- Callers must understand the abstraction's internals or special-case its output.
- The abstraction requires complex branching to support all consumers.

**True vs. coincidental duplication:** if one requirement change forces identical edits in both places, it is true duplication — unify. If the copies could evolve for different reasons, keep them separate.

Inline an abstraction when it stops paying for itself.

---

## Structure and Modularity

Structure code so behavior is understandable **locally**, changes have a **small blast radius**, side effects are **visible**, and dependencies are **intentional**.

### Functions
Keep functions small and single-purpose, at one level of abstraction, with few parameters. Use early returns and guard clauses to flatten nesting — the happy path stays un-nested at the left margin.

```python
# BAD: Deep nesting
def process_order(order):
    if order is not None:
        if order.is_paid:
            if order.in_stock:
                ship_item(order)

# GOOD: Guard clauses flatten logic
def process_order(order):
    if not order:
        return
    if not order.is_paid:
        return
    if not order.in_stock:
        return

    ship_item(order)
```

### Side Effects
Isolate I/O and state mutation from pure decision logic. Name side-effecting functions for what they change. Prefer computing data in pure functions, then acting on it at the boundaries.

```javascript
// BAD: Business logic mixed with I/O
async function calculateDiscountAndSave(userId, cartTotal) {
  const user = await db.users.find(userId);
  const discount = user.isVip ? cartTotal * 0.2 : 0;
  await db.orders.save({ userId, total: cartTotal - discount });
}

// GOOD: Pure logic separated from I/O at the boundary
function calculateDiscount(isVip, cartTotal) {
  return isVip ? cartTotal * 0.2 : 0;
}

async function checkout(userId, cartTotal) {
  const user = await db.users.find(userId);
  const discount = calculateDiscount(user.isVip, cartTotal);
  await db.orders.save({ userId, total: cartTotal - discount });
}
```

### Make Invalid States Unrepresentable
Validate at construction. Avoid half-initialized objects. Model data with the narrowest set of allowed values so illegal combinations cannot be built. Parse raw inputs into typed domain objects at the system boundary — once instantiated, internal invariants guarantee validity and eliminate defensive checks downstream.

```typescript
// BAD: Repeated validation of raw primitives
function processEmail(email: string) {
  if (!email.includes('@')) throw new Error("Invalid");
  // ...do work
}

// GOOD: Parse at the boundary, trust the type downstream
class Email {
  constructor(public readonly value: string) {
    if (!value.includes('@')) throw new Error("Invalid");
  }
}

function processEmail(email: Email) {
  // ...do work safely
}
```

### Deep Modules
Design modules that expose a narrow, simple interface while encapsulating complex implementation behind it. Pull complexity downward into the implementation so callers do not have to manage it.
- Do not create pass-through methods that simply forward arguments to another layer. These "shallow modules" scatter logic and increase cognitive load without adding value.
- Use exact types and domain objects over generic primitives at module boundaries to make incorrect usage syntactically impossible.
- Where possible, define errors out of existence: alter interface semantics so that edge cases are handled internally with safe default behavior rather than forcing callers to catch and handle exceptions.
- **Bend when:** the module is specifically designed as a thin adapter mapping one established protocol directly to another.

### Additional Structure Rules
- **Information hiding:** modules expose a minimal interface, hide internals.
- **Dependency direction:** point toward stable abstractions. High-level policy does not depend on low-level detail. No cycles.
- **Temporal coupling:** do not require callers to invoke things in a hidden order. Enforce setup via constructors/factories.
- **Local reasoning:** minimize shared mutable state, globals, singletons. Pass state explicitly.
- **Design for deletion:** a feature should be removable with limited edits elsewhere.
- **Composition over inheritance:** prefer composing small pieces over deep hierarchies.
- **Separation of concerns:** keep decisions, I/O, and presentation in different places.

---

## Error Handling

### Fail Fast vs. Recover
- **Unrecoverable errors** (missing critical config, corrupt state): fail immediately and loudly.
- **Transient errors** (network timeouts, locked records): return explicit error types or Result objects that force the caller to handle failure.
- Where interface design allows it, **define errors out of existence** — handle edge cases internally with safe, well-defined behavior so callers do not need to handle them. This complements fail-fast; it does not replace it for genuinely unrecoverable states.

### Rules
- Never catch-and-ignore. Catch only what you can handle or meaningfully translate.
- Preserve diagnostic context: include the operation, key inputs (**never secrets**), and the original cause.
- Keep error paths legible. Handle errors where you can act. Keep the happy path readable.
- Do not mask failure behind silent defaults or blanket retries.
- Model routine "absent/invalid" outcomes as return values. Reserve exceptions for the genuinely unexpected.
- Always release resources on failure (finally/defer/scope guards).
- Scale rigor to the stakes — but never allow a silent failure.
- Log errors at the highest logical boundary with structured contextual metadata. Do not log naked stack traces without domain context.
- When modifying existing code, preserve its error handling unless explicitly asked to change it.

---

## Comments and Documentation

- **Why, not what.** Write comments for rationale, constraints, assumptions, non-obvious tradeoffs. Code explains *what* — comments explain *why*.
- Delete comments that restate the code. Improve names and structure instead.
- Delete commented-out code. Version control remembers.
- Keep docs next to the code they describe.
- **Update or remove comments when behavior changes.** A stale comment is worse than none.
- Use `TODO`/`FIXME` sparingly. Do not let them accumulate.

---

## Code Comprehension Before Modification

Read and understand the environment before modifying it. Do not begin generating patches based on a naive keyword match from the task description.

- **Map the data flow** from entry point to exit point of the reported issue. Examine interacting modules, not just the file explicitly mentioned in the task.
- **Assess scope and risk** before writing any code. Identify what tests cover the area, what modules depend on the change, and whether the modification is localized or cross-cutting.
- **Do not confuse symptoms with root causes.** A bug's visible symptom often appears in a different module than the actual fault. Trace the causal chain before patching.
- If a change turns out to be significantly larger or riskier than the task implies, **stop and flag it** rather than forging ahead.
- **Bend when:** the issue is a strictly local, syntactic error (e.g., a typo, a missing import) that requires no cross-module reasoning.

---

## Refactoring and Scope Discipline

### Behavior Preservation
- **Preserve behavior by default.** Change structure, not observable behavior, unless the task says otherwise. Pin current behavior with tests or manual checks before starting.
- **Small steps:** one transformation at a time (rename → extract → move), each independently verifiable.
- **Do not mix refactor with feature/bugfix.** Separate commits. If a refactor enables the feature, land the refactor first.
- **Refactor for a reason:** reduce cognitive load, duplication, or coupling — not to conform to a pattern.
- Keep every step easy to review and easy to revert.

### Scope Discipline
Refactoring is not a license to rewrite. Protect the repository's audit trail.
- Restrict modifications strictly to the execution path of the requested feature or bug fix.
- Do not format, reorganize, or rename elements in parts of the file you were not asked to modify. Unsolicited "drive-by" refactoring destroys git blame history and inflates code reviews.
- **"Wash one more plate" rule:** you may make exactly *one* highly localized micro-improvement adjacent to your actual change (e.g., renaming a directly confusing variable, extracting one obviously long block). Stop there.
- If you discover pre-existing bugs or tech debt unrelated to the current task, document them (e.g., a TODO or a separate issue) — do not fix them in the same change.
- **Bend when:** the user explicitly requests a comprehensive refactoring, formatting, or linting pass.

---

## Security Baseline

These are **hard rules**, not heuristics:

- **Never** log secrets, tokens, credentials, or sensitive data.
- **Never** hardcode secrets, API keys, or credentials. Inject from environment variables or secret managers.
- **Never** weaken validation, authentication, authorization, or access controls.
- **Never** add a dependency, library, or package that is not already in the project's lockfile unless explicitly authorized by the user. Package hallucination (confidently referencing non-existent packages) is a known agent failure mode that creates supply-chain attack vectors.
- If a task requires external functionality not in the project, write the solution using the language's standard library, or vendor a minimal implementation into the source tree.
- Never execute raw package installation commands that alter lockfiles or fetch unpinned versions without explicit user authorization.
- Keep sensitive operations explicit and reviewable.
- If a task touches auth, secrets, user input, network boundaries, permissions, or sensitive data — treat it with extra care and prefer the most explicit approach.

---

## Testing Baseline

- Structure code to be testable: clear inputs/outputs, injectable dependencies, minimal global state.
- Do not make critical behavior hard to verify.

---

## Agent Execution Guardrails

Recognize when you are logically stuck and break the cycle. Do not fail expensively.

- **Track your own attempts.** If you fail to resolve a runtime or test error after two structurally distinct approaches, you are likely in a cognitive deadlock — repeatedly applying micro-variations of the same flawed logic.
- If a path fails twice, **step back and reframe**: explicitly identify the flawed assumption, then attempt a fundamentally different approach rather than another variation.
- Do not simplify complex conditionals, boundary checks, or edge-case handling just to make code shorter. Nuanced logic exists for a reason — preserve it during modifications.
- Do not confuse local and global scope. Before using a variable, verify it is accessible in the current scope.
- **Bend when:** the failure is a strict syntax, formatting, or type-checker error that requires iterative alignment rather than a change in logical approach.

---

## Anti-Patterns

Do not produce code exhibiting these patterns:

- **Clever code** — saves keystrokes, costs every future reader.
- **Premature abstraction** — indirection before the shape is known.
- **Speculative extensibility** — hooks for unbuilt futures (YAGNI).
- **Large multipurpose functions** — multiple jobs in one unit.
- **Hidden side effects** — state changes via globals/singletons.
- **Boolean flag arguments** — a flag that switches behavior is two functions; split them.
- **Inconsistent patterns** — multiple ways to do the same thing.
- **Excessive indirection** — many thin wrappers to trace one operation.
- **Shallow modules** — pass-through methods that forward arguments without adding value.
- **Over-generalized utility modules** — grab-bags without a clear responsibility.
- **Vague or misleading names** — `data`, `tmp`, `doProcess`.
- **Stale comments** — actively mislead readers.
- **Silent error swallowing** — failures resurface far from the cause.
- **Broad unsolicited rewrites** — risk regressions, hard to review, destroy git blame history.
- **Unnecessary dependencies** — maintenance and supply-chain cost for what existing code does.
- **Optimizing without evidence** — complexity for unverified gains.
- **Lava Flow** — dead or deprecated code left out of fear. Delete aggressively.
- **Cognitive deadlocks** — repeatedly applying micro-variations of a failed approach instead of stepping back to reframe.
- **Scope creep** — "improving" code beyond the task boundary, polluting diffs and destroying audit trails.

---

## Self-Review Checklist

Run this checklist before finalizing every change:

- [ ] **Intent** is clear from names and structure (domain vocabulary used).
- [ ] The solution is **simpler** than plausible alternatives.
- [ ] No **speculative** functionality was added.
- [ ] No **premature abstraction**; abstractions earned their place.
- [ ] Only **true duplication** (same knowledge, same change reason) was removed.
- [ ] **Side effects** are isolated from pure logic; invalid states are hard to represent.
- [ ] **Error paths** are clear; nothing is swallowed; context is preserved.
- [ ] **Existing behavior** is preserved unless the task asked to change it.
- [ ] The change is **small and reviewable**, nothing unrelated mixed in.
- [ ] **Scope is disciplined** — no drive-by refactoring beyond the task boundary.
- [ ] **Codebase conventions** were followed.
- [ ] No **unnecessary dependencies** introduced; no hallucinated packages.
- [ ] Critical behavior remains **testable**.
- [ ] **Security baseline** holds — no leaked secrets, no weakened auth/validation.
- [ ] The code can be **confidently modified later**.
- [ ] I am **not in a cognitive deadlock** — if stuck, I have reframed my approach.
