#!/usr/bin/env python3
"""Verify coverage exclusions against LLVM's function-level report."""
import json
import os
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parent.parent
fixture = root / "tests/coverage-fixture"
output = root / "target/coverage-regression"
output.mkdir(parents=True, exist_ok=True)
toolchain = os.environ.get("XDRS_COVERAGE_TOOLCHAIN", "nightly-2026-05-26")
env = dict(os.environ, RUSTFLAGS="--deny warnings")
reports = {}
for mode in ("included", "excluded", "nightly-cfg-disabled"):
    report = output / f"{mode}.json"
    cmd = ["cargo", f"+{toolchain}", "llvm-cov", "--manifest-path",
           str(fixture / "Cargo.toml"), "--lib", "--json", "--output-path", str(report)]
    if mode == "included":
        cmd += ["--features", "include-generated-coverage"]
    if mode == "nightly-cfg-disabled":
        cmd += ["--no-cfg-coverage-nightly"]
    subprocess.run(cmd, cwd=root, env=env, check=True)
    data = json.loads(report.read_text())
    reports[mode] = [f for d in data["data"] for f in d["functions"]
                     if any(Path(p) == fixture / "src/lib.rs" for p in f["filenames"])]

# Function names are Rust-mangled but retain these identifiers verbatim.
generated = ("read_xdr", "write_xdr", "type_def", "type_ident",
             "named_declaration", "spec", "const_write_", "const_cxs_size", "const_txs_size")
# rustc may omit mappings for some macro bodies, even when executed.
# The unused generic reader/writer mappings reproduce the reported issue.
for method in ("read_xdr", "write_xdr"):
    for mode in ("included", "nightly-cfg-disabled"):
        assert any(method in f["name"] for f in reports[mode]), (mode, method)
assert not any(any(m in f["name"] for m in generated) for f in reports["excluded"])
for mode, functions in reports.items():
    assert any("handwritten" in f["name"] and f["count"] > 0 for f in functions), mode
print("Generated methods are excluded by default; inclusion is opt-in; handwritten code stays covered.")
