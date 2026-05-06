# RPC Identifier Validation

**Date**: 2026-05-05
**Issue**: #85 — SunRPC Identifiers Are Emitted Raw

## Problem

`rpc::Id` is a public tuple struct with an unrestricted `String` field. The
SunRPC `Display` implementations interpolate identifier values directly into
grammar-significant positions (`program {} {`, `version {} {`, procedure
headers). Invalid or malicious identifier text produces malformed RFC 5531
output.

The XDR path already has `validate_xdr_identifier` (RFC 4506 §6.1 rules), but
the RPC path never calls it.

## Design

### Make `Id` opaque with a checked constructor

Change `pub struct Id(pub String)` to `pub struct Id(String)` — private inner
field.

Add:
- `Id::new(s: impl Into<String>)` — validates via `validate_xdr_identifier`,
  panics on invalid input (matches the existing `construct::ident()` pattern)
- `Id::as_str(&self) -> &str` — read access to the inner value

### Share the validator

`validate_xdr_identifier` currently lives in `xdr.rs` as `pub(crate)`. The RPC
module needs access to it. Either re-export it or move it to a shared utility
location within the crate.

### Display unchanged

`Display for Id` stays as-is — forwarding `self.0` is safe once `Id` can only
hold validated identifiers.

### Test updates

- `xdr-repr/tests/program.rs`: Change `Id("v0.1.0".into())` to
  `Id::new("v0_1_0")` and update the expected output string.
- The PoC from issue #85 should panic at construction (`Id::new("Bad-Program")`)
  instead of producing malformed IDL.

### Out of scope

- `Constant` — `usize`, no string injection risk.
- `Arg.name` — interpolated inside `/* */` comments; separate finding.
- `xdr::Ident(pub String)` — same structural issue but separate type; the XDR
  path already validates at usage sites. Hardening `Ident` the same way would be
  good follow-up work but is not part of this fix.

## Validation rules (RFC 4506 §6.1)

- Must not be empty
- First character: ASCII letter (`a-z`, `A-Z`) or underscore
- Remaining characters: ASCII letters, digits (`0-9`), or underscores
