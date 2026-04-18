# Agent Collaboration Manifest

## Active Agents
- **Agent 1** (Current): SturdyEngine Test Binary Fixer

## My Plans

### Task: Fix Test Binary and Use High-Level Engine API

**Current Issues:**
1. **Syntax error in main.rs** - Line with `[Phase 1]` is not valid Rust syntax (looks like a comment that was formatted incorrectly)
2. **Undefined identifiers** - `draw_pass`, `ImageBuilder::copy_image`, `ImageBuilder::blend_over`, `TextDrawDesc` are not real API items
3. **Slang shader file** - Contains backtick characters from Markdown formatting that corrupt the shader source
4. **API mismatch** - Code references methods/types that don't exist in the actual engine API

**Plan:**
1. Explore the actual engine API to understand what's available (DeviceManager, GraphFrame, ImageBuilder, passes, etc.)
2. Fix the slang shader file by removing Markdown backticks
3. Rewrite main.rs to use the real API correctly with proper types and method signatures
4. Ensure the test binary compiles and runs successfully

**Approach:**
- First, grep the engine source to find actual API definitions
- Then fix the shader file
- Finally, rewrite the test binary to match the real API

## Completed Work
- [ ] Explore engine API
- [ ] Fix slang shader file
- [ ] Rewrite main.rs
- [ ] Verify compilation