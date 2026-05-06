# RPC Identifier Validation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `rpc::Id` an opaque type with a validated constructor so that invalid identifiers are rejected at construction time, preventing malformed RPC-IDL output.

**Architecture:** Make the `Id` tuple field private, add `Id::new()` with validation via the existing `validate_xdr_identifier`, and add `Id::as_str()` for read access. Update all call sites from `Id("x".into())` to `Id::new("x")`.

**Tech Stack:** Rust, no new dependencies

---

### Task 1: Make `Id` opaque and add validated constructor

**Files:**
- Modify: `xdr-repr/src/rpc.rs:10-11`

- [ ] **Step 1: Write the failing test**

Add to `xdr-repr/tests/program.rs`:

```rust
#[test]
#[should_panic(expected = "Invalid XDR identifier")]
fn rpc_id_rejects_invalid_identifier() {
    Id::new("Bad-Program");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p xdr-repr --test program rpc_id_rejects_invalid_identifier`
Expected: FAIL — `Id::new` does not exist yet.

- [ ] **Step 3: Implement `Id` changes**

In `xdr-repr/src/rpc.rs`, change:

```rust
/// An identifier in an RPC program definition.
pub struct Id(pub String);
```

to:

```rust
use super::xdr::validate_xdr_identifier;

/// An identifier in an RPC program definition.
///
/// Validated at construction to contain only ASCII letters, digits,
/// and underscores per RFC 4506 §6.1.
pub struct Id(String);

impl Id {
    pub fn new(s: impl Into<String>) -> Self {
        let s = s.into();
        validate_xdr_identifier(&s);
        Self(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p xdr-repr --test program rpc_id_rejects_invalid_identifier`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add xdr-repr/src/rpc.rs xdr-repr/tests/program.rs
git commit -m "Make rpc::Id opaque with validated constructor (#85)"
```

### Task 2: Fix existing test to use valid identifier and `Id::new`

**Files:**
- Modify: `xdr-repr/tests/program.rs`

- [ ] **Step 1: Update the `show_prog` test**

In `xdr-repr/tests/program.rs`, change all `Id("x".into())` calls to `Id::new("x")`, and change the invalid version identifier `v0.1.0` to `v0_1_0`. The full updated test:

```rust
use xdr_repr::rpc::*;
use xdr_repr::xdr::TypeSpec;

#[test]
fn show_prog() {
    let proc1 = Procedure {
        id: Id::new("ping"),
        value: Constant(1),
        return_type: None,
        arg_types: vec![
            Arg {
                ty: TypeSpec::Int,
                name: "num".to_owned(),
            },
            Arg {
                ty: TypeSpec::Bool,
                name: "okay".to_owned(),
            },
        ],
    };

    let proc2 = Procedure {
        id: Id::new("pong"),
        value: Constant(2),
        return_type: Some(TypeSpec::Unsigned),
        arg_types: vec![],
    };

    let version10 = Version {
        id: Id::new("v0_1_0"),
        value: Constant(10),
        procedures: vec![proc1, proc2],
    };

    let program = Program {
        id: Id::new("PingPong"),
        value: Constant(42),
        versions: vec![version10],
    };

    let expected = "program PingPong {
\tversion v0_1_0 {
\t\tvoid ping (
\t\t\tint /* num */,
\t\t\tbool /* okay */
\t\t) = 1;
\t\tunsigned int pong (
\t\t) = 2;
\t} = 10;
} = 42;
";

    let output = program.to_string();
    assert_eq!(output, expected);
}

#[test]
#[should_panic(expected = "Invalid XDR identifier")]
fn rpc_id_rejects_invalid_identifier() {
    Id::new("Bad-Program");
}
```

- [ ] **Step 2: Run all tests in the program test file**

Run: `cargo test -p xdr-repr --test program`
Expected: Both `show_prog` and `rpc_id_rejects_invalid_identifier` PASS.

- [ ] **Step 3: Commit**

```bash
git add xdr-repr/tests/program.rs
git commit -m "Fix program test to use Id::new and valid identifier (#85)"
```

### Task 3: Update `Display for Id` to use `as_str`

**Files:**
- Modify: `xdr-repr/src/display_rpc.rs:6-9`

The current `Display for Id` accesses `self.0` directly, which works because `display_rpc` is a sibling module inside `xdr-repr`. However, since we made the field private, we should use the public accessor for consistency.

- [ ] **Step 1: Update `Display for Id`**

In `xdr-repr/src/display_rpc.rs`, change:

```rust
impl Display for Id {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        self.0.fmt(f)
    }
}
```

to:

```rust
impl Display for Id {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.write_str(self.as_str())
    }
}
```

- [ ] **Step 2: Run all tests**

Run: `cargo test -p xdr-repr --test program`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add xdr-repr/src/display_rpc.rs
git commit -m "Use Id::as_str in Display impl (#85)"
```

### Task 4: Full workspace build and test

- [ ] **Step 1: Build the full workspace**

Run: `RUSTFLAGS="--deny warnings" cargo build --workspace --all-features`
Expected: Clean build with no warnings or errors.

- [ ] **Step 2: Run all workspace tests**

Run: `RUSTFLAGS="--deny warnings" cargo test --workspace --all-features`
Expected: All tests pass.

- [ ] **Step 3: Commit (if any fixups needed)**

Only commit if earlier steps revealed issues that required fixes.
