#!/usr/bin/env python3
"""
Analyze the SturdyEngine codebase for:
1. Embedded test modules (#[cfg(test)]) that should be extracted to separate files
2. DRY opportunities in test code (repeated setup patterns, assertions, helpers)
3. DRY opportunities in runtime code (repeated patterns)
"""

import ast
import os
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path
from dataclasses import dataclass, field
from typing import Optional

ROOT = Path(__file__).parent.parent
CRATES = ROOT / "crates"

# ── Data structures ───────────────────────────────────────────────────────────

@dataclass
class TestModule:
    file: Path
    start_line: int
    end_line: int
    test_count: int
    lines: int

@dataclass
class FileStats:
    path: Path
    total_lines: int
    runtime_lines: int
    test_lines: int
    test_modules: list[TestModule] = field(default_factory=list)
    has_separate_test_file: bool = False

@dataclass
class DryPattern:
    pattern: str
    locations: list[tuple[Path, int]]
    category: str  # "test_setup", "assertion", "struct_literal", "match_arm", "runtime"

# ── Parsers ───────────────────────────────────────────────────────────────────

def find_cfg_test_blocks(text: str, path: Path) -> list[TestModule]:
    """Find all #[cfg(test)] mod ... { ... } blocks and their metadata."""
    modules = []
    lines = text.splitlines()

    i = 0
    while i < len(lines):
        line = lines[i].strip()

        # Look for #[cfg(test)] or #[test] on a standalone line
        if line == "#[cfg(test)]":
            # Find the mod ... { that follows
            j = i + 1
            while j < len(lines) and lines[j].strip() in ("", "//") or lines[j].strip().startswith("//"):
                j += 1
            if j < len(lines) and re.match(r'\s*(pub\s+)?mod\s+\w+\s*\{', lines[j]):
                start = i
                # Count braces to find end
                brace_depth = 0
                k = j
                test_count = 0
                while k < len(lines):
                    l = lines[k]
                    brace_depth += l.count('{') - l.count('}')
                    if re.search(r'#\[(?:tokio::)?test\]', l):
                        test_count += 1
                    if brace_depth == 0 and k > j:
                        end = k
                        modules.append(TestModule(
                            file=path,
                            start_line=start + 1,
                            end_line=end + 1,
                            test_count=test_count,
                            lines=end - start + 1,
                        ))
                        break
                    k += 1
        i += 1
    return modules


def analyze_file(path: Path) -> FileStats:
    """Analyze a single Rust source file."""
    try:
        text = path.read_text(encoding="utf-8")
    except Exception:
        return FileStats(path=path, total_lines=0, runtime_lines=0, test_lines=0)

    lines = text.splitlines()
    total = len(lines)

    test_modules = find_cfg_test_blocks(text, path)
    test_lines = sum(m.lines for m in test_modules)

    # Check for a separate tests file or integration test
    crate = path.parent
    stem = path.stem
    has_sep = (
        (crate / "tests" / f"{stem}_tests.rs").exists() or
        (crate / "tests" / f"{stem}.rs").exists() or
        (crate / "src" / "tests" / f"{stem}.rs").exists()
    )
    # Also check if there's a tests.rs in the same directory
    if not has_sep:
        has_sep = (path.parent / "tests.rs").exists() and path.stem != "tests"

    return FileStats(
        path=path,
        total_lines=total,
        runtime_lines=total - test_lines,
        test_lines=test_lines,
        test_modules=test_modules,
        has_separate_test_file=has_sep,
    )


# ── DRY pattern detection ─────────────────────────────────────────────────────

def extract_patterns(text: str, path: Path) -> list[tuple[str, int, str]]:
    """
    Extract repeating code patterns from a Rust file.
    Returns list of (pattern_text, line_number, category).
    """
    results = []
    lines = text.splitlines()

    # 1. PassDesc literal constructions (we already know these are 63+)
    for i, line in enumerate(lines, 1):
        stripped = line.strip()

        # Repeated struct literal openers
        if re.match(r'(crate::)?PassDesc\s*\{', stripped):
            results.append(("PassDesc{}", i, "struct_literal"))

        # Repeated ImageDesc::new() patterns
        if re.match(r'(crate::)?ImageDesc::new\(\)', stripped) or \
           re.match(r'\.\.(crate::)?ImageDesc::new\(\)', stripped):
            results.append(("..ImageDesc::new()", i, "struct_update"))

        # Repeated graph.add_pass calls
        if re.search(r'\.add_pass\(', stripped):
            results.append(("graph.add_pass()", i, "api_call"))

        # Repeated assert! patterns in tests
        if re.match(r'assert!\(matches!\(', stripped):
            results.append(("assert!(matches!(...))", i, "assertion"))
        if re.match(r'assert_eq!\(', stripped):
            results.append(("assert_eq!", i, "assertion"))

        # Repeated Error::Backend patterns
        if 'Error::Backend(format!(' in stripped:
            results.append(('Error::Backend(format!(...)', i, "error_pattern"))

        # Repeated expect("...mutex poisoned") patterns
        if 'expect("' in stripped and 'mutex poisoned' in stripped:
            results.append(("expect(\"...mutex poisoned\")", i, "mutex_unwrap"))

        # Repeated unsafe { device.xxx } in error closures
        if re.search(r'\.map_err\(\|e\|\s*\{', stripped):
            results.append(("map_err closure", i, "error_handling"))

        # Test fixture patterns - device/graph setup
        if re.search(r'let mut graph\s*=\s*RenderGraph::new\(\)', stripped):
            results.append(("RenderGraph::new() setup", i, "test_setup"))

        # Repeated validate_pipeline_layout calls in tests
        if re.search(r'validate_pipeline_layout\(', stripped):
            results.append(("validate_pipeline_layout()", i, "test_assertion"))

    return results


def find_dry_opportunities(files: list[FileStats]) -> dict[str, list[DryPattern]]:
    """Find the most impactful DRY opportunities across the codebase."""
    pattern_locations: dict[str, list[tuple[Path, int]]] = defaultdict(list)
    pattern_categories: dict[str, str] = {}

    for fs in files:
        if fs.total_lines == 0:
            continue
        try:
            text = fs.path.read_text(encoding="utf-8")
        except Exception:
            continue

        for pattern, line, category in extract_patterns(text, fs.path):
            pattern_locations[pattern].append((fs.path, line))
            pattern_categories[pattern] = category

    # Only keep patterns that appear in multiple files or many times in one file
    dry: dict[str, DryPattern] = {}
    for pattern, locations in pattern_locations.items():
        if len(locations) >= 3:  # Appears 3+ times = DRY opportunity
            dry[pattern] = DryPattern(
                pattern=pattern,
                locations=locations,
                category=pattern_categories.get(pattern, "unknown"),
            )

    # Group by category
    by_category: dict[str, list[DryPattern]] = defaultdict(list)
    for dp in dry.values():
        by_category[dp.category].append(dp)

    # Sort each category by occurrence count
    for cat in by_category:
        by_category[cat].sort(key=lambda d: len(d.locations), reverse=True)

    return dict(by_category)


# ── Specific repeated-block detection ────────────────────────────────────────

def find_repeated_test_helpers(all_texts: dict[Path, str]) -> list[tuple[str, list[Path]]]:
    """Find common test setup blocks that should be helper functions."""
    helpers = []

    # Pattern: "let mut graph = RenderGraph::new(); graph.import_image/buffer/..."
    graph_setup_files = []
    for path, text in all_texts.items():
        if "RenderGraph::new()" in text and "#[test]" in text:
            graph_setup_files.append(path)
    if len(graph_setup_files) > 2:
        helpers.append(("make_test_graph() helper", graph_setup_files))

    # Pattern: fn pass_with_work() inline in tests
    pass_with_work_files = []
    for path, text in all_texts.items():
        if "pass_with_work" in text and "#[cfg(test)]" in text:
            pass_with_work_files.append(path)
    if pass_with_work_files:
        helpers.append(("pass_with_work() (already exists in render_graph.rs, should be shared)", pass_with_work_files))

    # Pattern: PassDesc { name: ..., shading_rate_image: None, perf_counters: None, ... }
    passdesc_files = []
    for path, text in all_texts.items():
        count = text.count("perf_counters: None,")
        if count > 2:
            passdesc_files.append((path, count))
    if passdesc_files:
        helpers.append(("PassDesc::default_graphics(name) / PassDesc::default_compute(name) constructors",
                        [p for p, _ in sorted(passdesc_files, key=lambda x: -x[1])]))

    return helpers


# ── Reporting ─────────────────────────────────────────────────────────────────

def report(all_stats: list[FileStats], dry: dict, test_helpers: list) -> str:
    lines = []

    # ── Section 1: Files with large embedded test modules ────────────────────
    lines.append("=" * 72)
    lines.append("TEST EXTRACTION CANDIDATES (embedded test modules by size)")
    lines.append("=" * 72)
    lines.append(f"{'File':<55} {'Runtime':>8} {'Tests':>8} {'%Test':>7} {'Modules':>8}")
    lines.append("-" * 72)

    candidates = [
        fs for fs in all_stats
        if fs.test_lines > 50 and not fs.has_separate_test_file
    ]
    candidates.sort(key=lambda fs: fs.test_lines, reverse=True)

    for fs in candidates:
        rel = fs.path.relative_to(ROOT)
        pct = (fs.test_lines / fs.total_lines * 100) if fs.total_lines else 0
        mods = len(fs.test_modules)
        lines.append(f"{str(rel):<55} {fs.runtime_lines:>8} {fs.test_lines:>8} {pct:>6.0f}%  {mods:>7}")

    lines.append("")
    total_extractable = sum(fs.test_lines for fs in candidates)
    lines.append(f"Total extractable test lines: {total_extractable:,}")

    # ── Section 2: Detailed module info for top candidates ───────────────────
    lines.append("")
    lines.append("=" * 72)
    lines.append("TOP CANDIDATES — TEST MODULE DETAILS")
    lines.append("=" * 72)

    for fs in candidates[:10]:
        rel = fs.path.relative_to(ROOT)
        lines.append(f"\n  {rel}")
        for m in fs.test_modules:
            lines.append(f"    cfg(test) block: lines {m.start_line}–{m.end_line} "
                         f"({m.lines} lines, {m.test_count} tests)")

    # ── Section 3: DRY opportunities ─────────────────────────────────────────
    lines.append("")
    lines.append("=" * 72)
    lines.append("DRY OPPORTUNITIES")
    lines.append("=" * 72)

    cat_labels = {
        "struct_literal": "Struct literals (→ constructor helpers)",
        "struct_update": "Struct update syntax (→ already uses ..Default)",
        "test_setup": "Test setup boilerplate (→ test fixtures)",
        "test_assertion": "Test assertion patterns (→ assert helpers)",
        "assertion": "Assertion patterns",
        "error_pattern": "Error construction patterns (→ helper fns)",
        "mutex_unwrap": "Mutex unwrap patterns (→ helper macro)",
        "api_call": "API call patterns",
        "error_handling": "Error handling closures",
    }

    for cat, patterns in sorted(dry.items(), key=lambda kv: -sum(len(d.locations) for d in kv[1])):
        label = cat_labels.get(cat, cat)
        total_occurrences = sum(len(d.locations) for d in patterns)
        lines.append(f"\n  {label} — {total_occurrences} total occurrences")
        for dp in patterns[:5]:
            unique_files = len(set(p for p, _ in dp.locations))
            lines.append(f"    [{len(dp.locations):4d}× in {unique_files:3d} files] {dp.pattern}")

    # ── Section 4: Test helper functions needed ───────────────────────────────
    lines.append("")
    lines.append("=" * 72)
    lines.append("SUGGESTED NEW HELPERS / MACROS")
    lines.append("=" * 72)

    for name, affected_files in test_helpers:
        lines.append(f"\n  {name}")
        for f in affected_files[:5]:
            rel = f.relative_to(ROOT) if isinstance(f, Path) else f
            lines.append(f"    {rel}")
        if len(affected_files) > 5:
            lines.append(f"    ... and {len(affected_files) - 5} more")

    # ── Section 5: Summary ───────────────────────────────────────────────────
    lines.append("")
    lines.append("=" * 72)
    lines.append("SUMMARY")
    lines.append("=" * 72)
    total_rs = len(all_stats)
    files_with_tests = sum(1 for fs in all_stats if fs.test_lines > 0)
    total_test_lines = sum(fs.test_lines for fs in all_stats)
    total_runtime = sum(fs.runtime_lines for fs in all_stats)
    lines.append(f"  Rust files scanned:              {total_rs:6,}")
    lines.append(f"  Files with embedded tests:       {files_with_tests:6,}")
    lines.append(f"  Total embedded test lines:       {total_test_lines:6,}")
    lines.append(f"  Total runtime lines:             {total_runtime:6,}")
    lines.append(f"  Test / runtime ratio:            {total_test_lines/max(total_runtime,1)*100:6.1f}%")
    lines.append(f"  Extractable test lines:          {total_extractable:6,}")

    return "\n".join(lines)


# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    print("Scanning codebase...", flush=True)

    rs_files = []
    for root, dirs, files in os.walk(CRATES):
        # Skip target, generated, and test output dirs
        dirs[:] = [d for d in dirs if d not in {"target", ".git", "generated"}]
        for f in files:
            if f.endswith(".rs"):
                rs_files.append(Path(root) / f)

    print(f"  Found {len(rs_files)} Rust files", flush=True)

    all_stats = [analyze_file(p) for p in rs_files]
    all_stats.sort(key=lambda fs: fs.test_lines, reverse=True)

    # Load all texts for DRY analysis
    all_texts: dict[Path, str] = {}
    for p in rs_files:
        try:
            all_texts[p] = p.read_text(encoding="utf-8")
        except Exception:
            pass

    print("  Analyzing DRY opportunities...", flush=True)
    dry = find_dry_opportunities(all_stats)
    test_helpers = find_repeated_test_helpers(all_texts)

    print("  Generating report...", flush=True)
    print()
    print(report(all_stats, dry, test_helpers))

    # Write machine-readable extraction plan
    plan_path = Path(__file__).parent / "test_extraction_plan.txt"
    with open(plan_path, "w") as f:
        for fs in all_stats:
            if fs.test_lines > 50 and not fs.has_separate_test_file:
                rel = fs.path.relative_to(ROOT)
                f.write(f"{rel}|{fs.test_lines}|{fs.runtime_lines}\n")
                for m in fs.test_modules:
                    f.write(f"  MODULE|{m.start_line}|{m.end_line}|{m.test_count}\n")
    print(f"\nExtraction plan written to {plan_path}", flush=True)


if __name__ == "__main__":
    main()
