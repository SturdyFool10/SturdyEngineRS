# Production Panic Policy

> Roadmap reference: P0.R2

## Principle

A panic is only acceptable when the engine is in a state from which it is
**impossible to continue running correctly**.  Every other failure must be
returned as a structured diagnostic so callers can decide whether to degrade,
retry, report to the user, or shut down gracefully.

---

## Acceptable panics

The following are the only situations where a `panic!`, `.unwrap()`, or
`.expect()` is acceptable in production (non-test) code.

### 1. Hard incompatibility at startup

A condition that makes the process unable to run at all — for example, a
required GPU feature that the driver does not support, or a static C string
constant that contains an interior NUL byte.  These panics must occur during
initialization, not during a running frame or event loop.

```SturdyEngine/docs/panic_policy.md#L1-1
// Acceptable: static constant that must never contain NUL.
let name = CString::new("SturdyEngine").expect("static string has no nul");
```

### 2. Poisoned mutex / lock

When a `Mutex` or `RwLock` is poisoned it means another thread panicked while
holding the lock.  The data inside may be partially written and there is no
safe way to continue.  Treating a poisoned lock as an unrecoverable error is
therefore acceptable:

```SturdyEngine/docs/panic_policy.md#L1-1
// Acceptable: poisoned mutex means a previous thread already panicked.
let guard = self.inner.lock().expect("device mutex poisoned");
```

This is only acceptable for *internal* engine mutexes.  If a mutex guards
app-provided data, the engine must propagate a structured error instead.

### 3. Invariants that are impossible to violate by construction

A `debug_assert!` or `.expect()` that guards a data-structure invariant which
the implementation guarantees can never be broken — for instance, a non-empty
collection that is proven non-empty by construction.  The message must clearly
state the invariant.

---

## Not acceptable

The following are **not acceptable** in production code:

| Pattern | Required alternative |
|---|---|
| `.unwrap()` on a fallible operation whose failure is a runtime condition | Return `Result` or a structured diagnostic |
| `panic!("todo")` / `todo!()` / `unimplemented!()` | Implement the feature, or gate it behind a `Result::Err` until it is ready |
| `.expect(...)` on resource allocation, I/O, or asset loading | Return `Result` with `EngineError` or a backend-specific error variant |
| Panicking inside the render loop or event loop | Return an error to the caller so it can decide how to handle it |
| Panicking on recoverable Wayland/X11/compositor/platform failures | Return a degraded `WindowReconfigure` result |
| Panicking on missing or stale assets | Return `AssetState::Missing` / `AssetState::Stale` |
| Panicking on shader compilation failure at runtime | Return a compile error via `RuntimeController::report_shader_compile_error` |

---

## Error propagation model

Production code should use one of these approaches instead of panicking:

1. **`Result<T, EngineError>`** — the standard return type for engine operations
   that can fail due to driver, backend, platform, or resource conditions.

2. **Structured diagnostic** — for non-fatal conditions that the app should be
   informed about but that do not prevent the current operation from returning a
   usable value.  Use `RuntimeController::report_*`, `AssetState`, or
   `RuntimeChangeResult::Degraded / Rejected / Unavailable`.

3. **`Option<T>`** — for "not found" / "not yet available" states where `None`
   is a first-class outcome, not an error.

---

## CI enforcement

Run `python3 tools/panic-audit.py` as part of CI.  The script:

- Exits `0` when all production panics are marked, or no production panics are found.
- Exits `1` and prints a categorized report when unmarked production panics are present.
- Exits `2` on script-level errors (missing directory, etc.).

Any new `.unwrap()`, `.expect()`, `panic!()`, `todo!()`, or `unimplemented!()`
added to production code should be justified in the PR description and reviewed
against this policy.  Panics that meet the acceptable criteria above should be
preceded by a marker comment with a short reason. The marker applies only to the
next statement and does not suppress later panic sites:

```SturdyEngine/docs/panic_policy.md#L1-1
//panic allowed, reason = "poisoned mutex is unrecoverable"
let guard = lock.lock().expect("scheduler mutex poisoned");
```

---

## Current state

Run `python3 tools/panic-audit.py` for a live count.  The two main categories
currently in the codebase are:

- **Poisoned mutex guards** — acceptable per this policy; tracked for future
  cleanup if the locking strategy changes.
- **`.unwrap()` / `.expect()` on fallible runtime operations** — these should
  be converted to `Result`-returning code incrementally, tracked by
  roadmap items P0.R3 and P0.R4.
