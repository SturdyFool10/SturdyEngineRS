#!/usr/bin/env python3
"""
Extract embedded #[cfg(test)] modules from source files into separate test files.

For each target file, the #[cfg(test)] mod block is moved to:
  <file_dir>/<file_stem>/tests.rs  (for files like foo.rs → foo/tests.rs)
  OR
  <file_dir>/tests.rs              (for mod.rs files → tests.rs sibling)

The source file gets:
  #[cfg(test)]
  mod tests;
in place of the original block.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).parent.parent

# ── Targets ────────────────────────────────────────────────────────────────────
# (source_file_rel, test_file_rel)
TARGETS = [
    # Core render graph — 1674 test lines, 50% of file
    (
        "crates/sturdy-engine-core/src/render_graph.rs",
        "crates/sturdy-engine-core/src/render_graph/tests.rs",
    ),
    # Alias plan — 388 test lines, 54% of file
    (
        "crates/sturdy-engine-core/src/render_graph/alias_plan.rs",
        "crates/sturdy-engine-core/src/render_graph/alias_plan_tests.rs",
    ),
    # Slang — 365 test lines, 23% of file
    (
        "crates/sturdy-engine-core/src/slang.rs",
        "crates/sturdy-engine-core/src/slang/tests.rs",
    ),
    # Backend features — 245 test lines, 45% of file
    (
        "crates/sturdy-engine-core/src/backend_features.rs",
        "crates/sturdy-engine-core/src/backend_features_tests.rs",
    ),
    # ECS mod — 336 test lines, 76% of file (mod.rs → sibling tests.rs)
    (
        "crates/sturdy-engine/src/ecs/mod.rs",
        "crates/sturdy-engine/src/ecs/tests.rs",
    ),
    # Allocator — small but frequently-accessed
    (
        "crates/sturdy-engine-core/src/backend/vulkan/allocator.rs",
        "crates/sturdy-engine-core/src/backend/vulkan/allocator_tests.rs",
    ),
    # Colorlab lib — 92% tests (242/263 lines)
    (
        "crates/colorlab/src/lib.rs",
        "crates/colorlab/src/tests.rs",
    ),
    # Image — 90 test lines
    (
        "crates/sturdy-engine-core/src/image.rs",
        "crates/sturdy-engine-core/src/image_tests.rs",
    ),
    # Error — 51 test lines
    (
        "crates/sturdy-engine-core/src/error.rs",
        "crates/sturdy-engine-core/src/error_tests.rs",
    ),
    # External resource — 54 test lines, 43% of file
    (
        "crates/sturdy-engine-core/src/external_resource.rs",
        "crates/sturdy-engine-core/src/external_resource_tests.rs",
    ),
    # Backend vulkan resources — 120 test lines
    (
        "crates/sturdy-engine-core/src/backend/vulkan/resources.rs",
        "crates/sturdy-engine-core/src/backend/vulkan/resources_tests.rs",
    ),
]


def find_cfg_test_block(text: str):
    """
    Find the #[cfg(test)] ... mod tests { ... } block.
    Returns (start_idx_in_text, end_idx_in_text, mod_name) or None.
    """
    # Find #[cfg(test)] that precedes a mod declaration
    pattern = re.compile(
        r"(#\[cfg\(test\)\]\s*)"  # attribute
        r"((?:#\[.*?\]\s*)*)"  # additional attributes
        r"(pub\s+)?mod\s+(\w+)\s*\{",  # mod name and opening brace
        re.DOTALL,
    )
    for m in pattern.finditer(text):
        start = m.start()
        brace_start = m.end() - 1  # points at the '{'
        # Count braces to find the matching '}'
        depth = 0
        i = brace_start
        while i < len(text):
            if text[i] == "{":
                depth += 1
            elif text[i] == "}":
                depth -= 1
                if depth == 0:
                    end = i + 1
                    mod_name = m.group(4)
                    return start, end, mod_name
            i += 1
    return None


def extract_mod_body(text: str, block_start: int, block_end: int, mod_name: str) -> str:
    """Extract the body of the mod block, stripping the outer braces."""
    # Find the opening { of the mod
    header_end = text.index("{", block_start) + 1
    body = text[header_end : block_end - 1]  # strip outer { }
    # Un-indent one level (4 spaces)
    lines = body.splitlines(keepends=True)
    unindented = []
    for line in lines:
        if line.startswith("    "):
            unindented.append(line[4:])
        elif line.startswith("\t"):
            unindented.append(line[1:])
        else:
            unindented.append(line)
    return "".join(unindented)


def build_test_file_content(
    body: str,
    source_rel: str,
    mod_name: str,
    use_super: bool,
) -> str:
    """Build the content for the extracted test file."""
    parts = [
        f"// Tests extracted from {source_rel}\n",
        "// See scripts/extract_tests.py for the extraction logic.\n\n",
    ]
    if use_super:
        parts.append("use super::*;\n\n")
    parts.append(body.strip() + "\n")
    return "".join(parts)


def process(src_rel: str, dst_rel: str, dry_run: bool = False) -> bool:
    """Extract the test module from src to dst. Returns True on success."""
    src = ROOT / src_rel
    dst = ROOT / dst_rel

    if not src.exists():
        print(f"  SKIP (source not found): {src_rel}")
        return False
    if dst.exists():
        print(f"  SKIP (destination exists): {dst_rel}")
        return False

    text = src.read_text(encoding="utf-8")
    result = find_cfg_test_block(text)
    if result is None:
        print(f"  SKIP (no #[cfg(test)] mod found): {src_rel}")
        return False

    block_start, block_end, mod_name = result

    # Extract body
    body = extract_mod_body(text, block_start, block_end, mod_name)

    # Determine whether the test file should have `use super::*`
    # (it already has it if the original mod body has it)
    has_use_super = "use super::*" in body

    # Build new test file content
    test_content = build_test_file_content(
        body=body,
        source_rel=src_rel,
        mod_name=mod_name,
        use_super=not has_use_super,  # add it if missing
    )

    # Build replacement text in source file
    # The mod declaration in the source is replaced with:
    # #[cfg(test)]
    # mod <modname>;
    replacement = f"#[cfg(test)]\nmod {mod_name};\n"

    new_source = text[:block_start] + replacement + text[block_end:]
    # Clean up double blank lines that might result
    new_source = re.sub(r"\n{3,}", "\n\n", new_source)

    if dry_run:
        print(f"  DRY RUN: would extract {block_end - block_start} chars → {dst_rel}")
        return True

    # Write files
    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.write_text(test_content, encoding="utf-8")
    src.write_text(new_source, encoding="utf-8")
    lines_moved = body.count("\n")
    print(f"  OK  {src_rel} → {dst_rel}  ({lines_moved} lines moved, mod '{mod_name}')")
    return True


def main():
    dry = "--dry-run" in sys.argv or "-n" in sys.argv
    if dry:
        print("DRY RUN — no files will be modified\n")

    ok = 0
    skip = 0
    for src_rel, dst_rel in TARGETS:
        if process(src_rel, dst_rel, dry_run=dry):
            ok += 1
        else:
            skip += 1

    print(f"\nDone: {ok} extracted, {skip} skipped")
    if not dry:
        print("\nRun `cargo build` to verify the extractions compiled correctly.")


if __name__ == "__main__":
    main()
