# `get_type_json` Feature Documentation

## Overview

The `get_type_json()` methods on `Declaration` and `TypeSpec` provide a way to introspect XDR type structures by generating JSON-like string representations.

**Location:** `xdr-repr/src/xdr.rs:203-302`

## API

### `Declaration::get_type_json(&self) -> String`

Returns a JSON-like representation of the declaration's type structure (excluding the name).

**Example outputs:**
```json
{ "type": "void" }
{ "type": "int" }
{ "type": "fixed opaque", "len": 10 }
{ "type": "variable opaque", "max": 100 }
{ "type": "string", "max": 50 }
{ "type": "fixed array", "elt_ty": unsigned, "len": 5 }
{ "type": "variable array", "elt_ty": int, "max": 10 }
{ "type": "optional", "opt_ty": bool }
```

### `TypeSpec::get_type_json(&self) -> String`

Returns the type name as a string (unquoted) for embedding in `Declaration::get_type_json()` output.

**Example outputs:**
- Primitives: `"int"`, `"unsigned"`, `"hyper"`, `"unsigned hyper"`, `"float"`, `"double"`, `"quadruple"`, `"bool"`
- Named types: Returns the identifier name (e.g., `"CustomType"`)

## Known Issues and Limitations

### 🚨 CRITICAL ISSUES

#### 1. **NOT Valid JSON** (Severity: HIGH)

The output is **NOT** valid JSON and cannot be parsed by standard JSON parsers.

**Problem:**
- String values are not quoted: `{ "type": "fixed array", "elt_ty": unsigned, "len": 5 }`
- Should be: `{ "type": "fixed array", "elt_ty": "unsigned", "len": 5 }`

**Impact:**
- Cannot use `JSON.parse()` or similar parsers
- Only useful for human-readable output or simple string matching

**Workaround:**
- Treat as a debugging/logging format only
- Do NOT attempt to parse as JSON

**Location:** `xdr-repr/src/xdr.rs:203-302`

#### 2. **Panics on Embedded Types** (Severity: HIGH)

`TypeSpec::get_type_json()` panics for embedded enum/struct/union types.

**Problem:**
```rust
TypeSpec::Enum(_body) => panic!("Embedded enum type not supported"),
TypeSpec::Struct(_body) => panic!("Embedded struct type not supported"),
TypeSpec::Union(_body) => panic!("Embedded union type not supported"),
```

**Impact:**
- Will crash the program if called on declarations with embedded types
- No error handling or graceful degradation

**Workaround:**
- Ensure all enums/structs/unions are defined at the top level
- Check type before calling `get_type_json()`

**Location:** `xdr-repr/src/xdr.rs:296-298`

### ⚠️ MODERATE ISSUES

#### 3. **No String Escaping** (Severity: MEDIUM)

Identifiers are inserted directly without escaping special characters.

**Problem:**
```rust
TypeSpec::Ident(ident) => ident.0.clone(),
```

**Impact:**
- If an identifier contains quotes or special characters, the output will be malformed
- Example: identifier `My"Type` would produce: `{ "type": "My"Type" }`

**Workaround:**
- Avoid identifiers with special characters

**Location:** `xdr-repr/src/xdr.rs:299`

#### 4. **Implicit Display Dependency** (Severity: LOW)

The JSON output relies on the `Display` implementation of `Value` and `Constant`.

**Problem:**
- Format strings like `"len\": {}"` depend on how `Value::fmt()` is implemented
- Changes to Display could break the JSON-like output
- Not immediately obvious from reading the code

**Impact:**
- Fragile coupling between formatting logic and JSON generation
- Hard to maintain

**Location:** `xdr-repr/src/xdr.rs:210, 213, 216, etc.` and `xdr-repr/src/format.rs:175-190`

#### 5. **Inconsistent Formatting** (Severity: LOW)

The `Void` case uses a string literal while others use `format!` macro.

**Problem:**
```rust
Declaration::Void => {
    "{ \"type\": \"void\" }".to_string()
}
```

vs.

```rust
Declaration::Named { name: _, ty } => {
    format!("{{ \"type\": \"{}\" }}", ty.get_type_json())
}
```

**Impact:**
- Stylistic inconsistency
- Could lead to bugs if the format changes

**Note:** This was recently fixed in commit #13 to properly handle the void case.

**Location:** `xdr-repr/src/xdr.rs:246-248`

## Use Cases

### ✅ Recommended Use Cases

1. **Debugging and logging** - Quick inspection of type structures
2. **Simple type introspection** - String-based pattern matching for type information
3. **Human-readable output** - Showing type info in error messages or debug output

### ❌ NOT Recommended Use Cases

1. **Serialization** - Do NOT use for serializing XDR type information
2. **JSON parsing** - Do NOT attempt to parse with JSON libraries
3. **API responses** - Do NOT send this to external systems expecting valid JSON
4. **Type information interchange** - Do NOT use for exchanging type metadata between systems

## Recommended Improvements

If this feature needs to be production-ready, consider:

1. **Make it valid JSON:**
   - Properly quote all string values
   - Escape special characters in identifiers
   - Use a proper JSON serialization library (e.g., `serde_json` with `no_std`)

2. **Add error handling:**
   - Return `Result<String, Error>` instead of panicking
   - Handle embedded types gracefully (serialize them or return an error)

3. **Add a feature flag:**
   - Put this behind a feature flag like `type-introspection` or `json-repr`
   - Make it opt-in since it's not core XDR functionality

4. **Consider using serde:**
   ```rust
   #[cfg(feature = "serde")]
   impl Serialize for Declaration { ... }
   ```
   This would provide proper JSON serialization with all the edge cases handled.

## Testing

The feature has basic test coverage in `xdr-repr/tests/declaration_json.rs`:

- ✅ Void declaration
- ✅ Named declaration
- ✅ Fixed opaque
- ✅ Variable opaque
- ✅ String
- ✅ Fixed array
- ✅ Optional

**Missing test coverage:**
- ❌ Variable array
- ❌ Nested/complex types
- ❌ Identifiers with special characters
- ❌ Embedded types (panic behavior)

## References

- **Implementation:** `xdr-repr/src/xdr.rs:203-302`
- **Tests:** `xdr-repr/tests/declaration_json.rs`
- **Related commit:** #13 (fix json spec gen for void case)
