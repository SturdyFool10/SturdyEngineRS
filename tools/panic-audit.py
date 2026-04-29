#!/usr/bin/env python3
"""panic-audit: find potentially unsafe panicking calls in production Rust source.

Scans every .rs file under crates/ (excluding target/ and generated files),
reports lines containing .unwrap(), .expect(, panic!(, todo!(, and
unimplemented!(  that are NOT inside #[cfg(test)] blocks, #[test] functions,
mod tests { } blocks, or explicitly reviewed `//panic allowed` sites.

Usage
-----
  python3 tools/panic-audit.py [--root <project-root>] [--json]

  --root   Path to the project root (default: directory containing this script's
           parent, i.e. the repo root).
  --json   Emit newline-delimited JSON instead of human-readable text.

Exit codes
----------
  0  All production panics are marked, or no production panics were found.
  1  At least one unmarked production panic found.
  2  Script error (bad arguments, directory not found, etc.).
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

# ── Patterns to flag ──────────────────────────────────────────────────────────

PANIC_RE = re.compile(
    r"""
    (?:
        \.unwrap\(\)          # .unwrap()
      | \.expect\s*\(         # .expect(
      | \bpanic!\s*\(         # panic!(
      | \btodo!\s*\(          # todo!(
      | \bunimplemented!\s*\( # unimplemented!(
    )
    """,
    re.VERBOSE,
)

# ── Test-context detection helpers ───────────────────────────────────────────

# Attributes / decorators that start a test scope on the *next* item.
TEST_SCOPE_ATTR_RE = re.compile(
    r"""
    \#\s*\[\s*
    (?:
        cfg\s*\(\s*test\s*\)   # #[cfg(test)]
      | test\b                  # #[test]
    )
    \s*\]
    """,
    re.VERBOSE,
)

# `//panic allowed, reason = "<reason>"` — marks a reviewed panic site.
ALLOW_PANIC_COMMENT_RE = re.compile(
    r'//\s*panic\s+allowed\s*,\s*reason\s*=\s*"[^"]+"'
)

# A `mod tests {` (or `mod test {`) line that opens a test module.
TEST_MOD_OPEN_RE = re.compile(
    r"""
    (?:^|\s)
    (?:pub\s+)?mod\s+tests?\s*\{
    """,
    re.VERBOSE,
)

# ── File-level exclusions ─────────────────────────────────────────────────────

# Files whose entire content should be treated as test/generated code.
EXCLUDED_FILE_PATTERNS = [
    re.compile(r"[\\/]tests?\.rs$"),  # tests.rs / test.rs
    re.compile(r"[\\/]tests?[\\/]"),  # tests/ subdirectories
    re.compile(r"_test(?:s)?\.rs$"),  # foo_tests.rs / foo_test.rs
    re.compile(r"[\\/]build\.rs$"),  # build scripts
    re.compile(r"[\\/]target[\\/]"),  # compiled output
    re.compile(r"\.generated\.rs$"),  # generated files
    re.compile(r"_generated\.rs$"),
]

# Crates whose output is deliberately FFI/C-facing boilerplate.
EXCLUDED_CRATE_PATTERNS = [
    re.compile(r"[\\/]sturdy-engine-ffi[\\/]"),
    re.compile(r"[\\/]sturdy-engine-macros[\\/]"),
]


def is_excluded_file(path: Path) -> bool:
    s = str(path)
    for p in EXCLUDED_FILE_PATTERNS + EXCLUDED_CRATE_PATTERNS:
        if p.search(s):
            return True
    return False


# ── Comment-stripping (line-level) ────────────────────────────────────────────

# We strip //-style line comments and /* */ block comments before matching
# patterns so that commented-out panics do not appear in the report.
# String literals are NOT stripped; panics inside string literals are rare
# and better caught by human review.


def strip_line_comment(line: str) -> str:
    """Remove everything from // to end of line (naive, ignores strings)."""
    # A very simple heuristic: find // that is not preceded by an odd number
    # of slashes (to avoid stripping inside raw strings like r"//").
    idx = line.find("//")
    if idx == -1:
        return line
    # Skip if it looks like it's inside a string (both sides have a quote).
    # This is a best-effort heuristic; a full Rust parser is out of scope.
    before = line[:idx]
    if before.count('"') % 2 == 1:
        return line  # inside a string literal, don't strip
    return line[:idx]


# ── Core scanner ─────────────────────────────────────────────────────────────


@dataclass
class Finding:
    path: str  # relative path from project root
    crate: str  # crate name
    line_no: int  # 1-based
    col: int  # 0-based column of the match
    line: str  # original (un-stripped) source line
    kind: str  # the matched keyword, e.g. ".unwrap()"


def _crate_name(rs_path: Path, crates_root: Path) -> str:
    """Return the crate directory name for a given .rs file path."""
    try:
        rel = rs_path.relative_to(crates_root)
        return rel.parts[0]
    except ValueError:
        return "<unknown>"


def scan_file(path: Path, crates_root: Path) -> list[Finding]:
    """Scan a single .rs file and return all production-panic findings."""
    try:
        source = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return []

    findings: list[Finding] = []
    lines = source.splitlines()

    # ── State machine for test-block tracking ────────────────────────────────
    # We track a stack of brace depths at which we entered a test scope.
    # When the current brace depth falls below a stack entry we pop it.
    #
    # Brace tracking is simplified: we count raw { and } characters after
    # stripping line comments.  String literals can contain braces, so this
    # is an approximation, but it is accurate enough for the audit purpose.

    brace_depth: int = 0
    # Stack of (entry_depth) for each test scope entered.
    test_scope_stack: list[int] = []
    # Pending flag: a #[cfg(test)] or #[test] attribute was seen; the next
    # item that opens a { starts a test scope.
    test_attr_pending: bool = False

    # panic-allowed suppression window.
    # Set when a standalone `//panic allowed, reason = "..."`
    # marker is seen; cleared at the first `;` that ends the following
    # statement, or when the enclosing scope closes.
    allow_panic_pending: bool = False
    allow_panic_at_depth: int = -1

    # Block-comment tracking across lines.
    in_block_comment: bool = False

    crate = _crate_name(path, crates_root)
    rel_path = str(path.relative_to(crates_root.parent))

    for line_idx, raw_line in enumerate(lines):
        line_no = line_idx + 1

        # ── Strip block comments ─────────────────────────────────────────────
        work = raw_line
        if in_block_comment:
            end = work.find("*/")
            if end == -1:
                # Still inside block comment, skip entirely.
                continue
            work = work[end + 2 :]
            in_block_comment = False

        # Remove /* ... */ spans within this line.
        while True:
            start = work.find("/*")
            if start == -1:
                break
            end = work.find("*/", start + 2)
            if end == -1:
                work = work[:start]
                in_block_comment = True
                break
            work = work[:start] + " " * (end + 2 - start) + work[end + 2 :]

        panic_allowed_comment = ALLOW_PANIC_COMMENT_RE.search(work) is not None
        marker_is_standalone = panic_allowed_comment and work.lstrip().startswith("//")

        # Strip line comment.
        work = strip_line_comment(work)

        # ── Detect //panic allowed markers ───────────────────────────────────
        if marker_is_standalone:
            allow_panic_pending = True
            allow_panic_at_depth = brace_depth

        # ── Detect test attribute ─────────────────────────────────────────────
        if TEST_SCOPE_ATTR_RE.search(work):
            test_attr_pending = True

        # ── Detect test mod open ──────────────────────────────────────────────
        if TEST_MOD_OPEN_RE.search(work):
            # Count the opening brace as part of depth update below.
            test_attr_pending = True  # treat as entering a test scope

        # ── Update brace depth ───────────────────────────────────────────────
        opens = work.count("{")
        closes = work.count("}")

        if opens > 0 and test_attr_pending:
            # The first opening brace on a pending-attr line enters the scope.
            test_scope_stack.append(brace_depth)
            test_attr_pending = False

        brace_depth += opens
        brace_depth -= closes

        # Pop test scopes that have been closed.
        while test_scope_stack and brace_depth <= test_scope_stack[-1]:
            test_scope_stack.pop()

        # ── Check for panic patterns ──────────────────────────────────────────
        # This must happen BEFORE closing the allow_panic window below so that
        # a `;` on the same line as `.expect()` is still suppressed:
        #
        #   //panic allowed, reason = "r" <- pending set
        #   let x = mutex.lock()          <- no `;`, pending stays
        #       .expect("p");             <- panic checked (suppressed), THEN `;` closes window
        if test_scope_stack or allow_panic_pending or panic_allowed_comment:
            # Inside a test scope or a reviewed panic-allowed statement — skip.
            pass  # fall through to window-close logic below
        else:
            for m in PANIC_RE.finditer(work):
                kind = m.group(0).strip()
                findings.append(
                    Finding(
                        path=rel_path,
                        crate=crate,
                        line_no=line_no,
                        col=m.start(),
                        line=raw_line.rstrip(),
                        kind=kind,
                    )
                )

        # ── Close the allow_panic window ─────────────────────────────────────
        # Runs AFTER the panic check so that the `;` terminating the statement
        # and a panic pattern on the same line are both correctly suppressed.
        # The window closes when:
        #   a) a `;` ends the annotated statement (single-line or last line of
        #      a multi-line chain), or
        #   b) the enclosing scope closes (tail-expression / no `;` case).
        if allow_panic_pending:
            if ";" in work or brace_depth < allow_panic_at_depth:
                allow_panic_pending = False

    return findings


def scan_crates(crates_root: Path) -> list[Finding]:
    """Walk crates_root and scan every eligible .rs file."""
    all_findings: list[Finding] = []
    for rs_path in sorted(crates_root.rglob("*.rs")):
        if is_excluded_file(rs_path):
            continue
        all_findings.extend(scan_file(rs_path, crates_root))
    return all_findings


# ── Reporting ─────────────────────────────────────────────────────────────────


def _kind_label(kind: str) -> str:
    if kind.startswith(".unwrap"):
        return "unwrap"
    if kind.startswith(".expect"):
        return "expect"
    if kind.startswith("panic"):
        return "panic!"
    if kind.startswith("todo"):
        return "todo!"
    if kind.startswith("unimplemented"):
        return "unimplemented!"
    return kind


def report_text(findings: list[Finding]) -> None:
    if not findings:
        print("✓  No unmarked production panics found.")
        return

    # Group by crate for the summary.
    by_crate: dict[str, list[Finding]] = {}
    for f in findings:
        by_crate.setdefault(f.crate, []).append(f)

    # Per-file detail.
    current_file = None
    for f in findings:
        if f.path != current_file:
            print(f"\n{f.path}")
            current_file = f.path
        label = _kind_label(f.kind)
        print(f"  {f.line_no:>5}  [{label:<15}]  {f.line.strip()}")

    # Summary table.
    print("\n" + "─" * 60)
    print(f"{'Crate':<40}  {'Count':>5}")
    print("─" * 60)
    total = 0
    for crate, crate_findings in sorted(by_crate.items()):
        print(f"  {crate:<38}  {len(crate_findings):>5}")
        total += len(crate_findings)
    print("─" * 60)
    print(f"  {'TOTAL':<38}  {total:>5}")
    print()


def report_json(findings: list[Finding]) -> None:
    for f in findings:
        print(
            json.dumps(
                {
                    "path": f.path,
                    "crate": f.crate,
                    "line": f.line_no,
                    "col": f.col,
                    "kind": _kind_label(f.kind),
                    "source": f.line.strip(),
                }
            )
        )


# ── Entry point ───────────────────────────────────────────────────────────────


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Audit production Rust source for panicking calls.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--root",
        default=None,
        help="Project root directory (default: parent of the tools/ directory).",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Emit newline-delimited JSON instead of human-readable text.",
    )
    args = parser.parse_args(argv)

    if args.root:
        root = Path(args.root).resolve()
    else:
        # Default: parent of the directory this script lives in.
        root = Path(__file__).resolve().parent.parent

    crates_root = root / "crates"
    if not crates_root.is_dir():
        print(f"error: crates/ directory not found under {root}", file=sys.stderr)
        return 2

    findings = scan_crates(crates_root)

    if args.json:
        report_json(findings)
    else:
        report_text(findings)

    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
