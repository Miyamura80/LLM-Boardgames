# Test Coverage Analysis

## Current State

### Rust Backend: 32 tests across 3 files

| File | Tests | Coverage Assessment |
|------|-------|-------------------|
| `global_config.rs` | 8 | Good - covers loading, env overrides, type coercion, sanitization |
| `logging.rs` | 22 | Good - covers session ID, redaction writer, log level determination, unicode, edge cases |
| `asset_gen.rs` | 2 | Minimal - only covers `remove_greenscreen` and `ensure_square` |
| `lib.rs` | 0 | None |

### Frontend: 0 tests, no framework configured

No test runner (Vitest, Jest, etc.) is installed. No test files exist.

---

## Gap Analysis and Recommendations

### Priority 1: Frontend Testing Infrastructure

**Problem:** Zero frontend test coverage. No test framework is even configured.

**Recommendation:** Set up Vitest (natural fit since the project already uses Vite) with `@testing-library/react` and `happy-dom`.

**Specific tests to add:**

- **`useConfig` hook** (`src/hooks/useConfig.ts`): This is the primary bridge between the Rust backend and React frontend. Tests should cover:
  - Initial state (`loading: true`, `config: null`, `error: null`)
  - Successful config fetch (mock `invoke`, verify config is set and loading becomes false)
  - Error handling (mock `invoke` rejection, verify error state)
  - Hook only calls `invoke` once on mount (empty dependency array)

- **`App` component** (`src/App.tsx`): The main component has untested interactive behavior:
  - Form submission calls `invoke("greet", { name })` and displays the result
  - Conditional rendering of `config.model_name` when config loads
  - The `greet` function has no error handling - if `invoke` rejects, the component will throw. This is a bug that tests would surface.

**Estimated scope:** ~4 test files, ~15-20 test cases.

---

### Priority 2: `asset_gen.rs` Image Processing Functions

**Problem:** `asset_gen.rs` is the most complex file (~540 lines) but has only 2 trivial tests. The image processing functions contain real algorithmic logic that is easy to get wrong.

**Specific tests to add:**

- **`remove_greenscreen` edge cases:**
  - Non-green pixels should be untouched (red, blue, white, black)
  - Near-green pixels at the tolerance boundary (e.g., `[0, 181, 0]` vs `[0, 179, 0]`)
  - Pixels with green tint that are visible (`a > 128`) should get de-tinted, not made transparent
  - Already-transparent pixels should remain transparent
  - Mixed-color pixels where green is high but not dominant (e.g., `[170, 200, 170]`)

- **`ensure_square` edge cases:**
  - Already-square image returns same dimensions
  - Wide image (landscape) gets vertical padding
  - 1x1 pixel image
  - Verify the content is centered (check pixel at expected offset)

- **`extract_text` and `extract_first_image`:**
  - Empty candidates list returns `None`
  - Candidate with no text parts returns `None`
  - Multiple candidates - returns first match
  - Invalid base64 in image data returns `None` (graceful failure)
  - Non-image mime type is skipped

- **`inline_image_from_rgba`:**
  - Produces valid PNG bytes (can be decoded back)
  - Correct mime type in output

- **`GenerateContentRequest` builders:**
  - `new_text` sets `response_modalities` to `["TEXT"]`
  - `new_image` sets modalities to `["IMAGE", "TEXT"]`
  - `new_image_with_ref` includes the inline image data

**Estimated scope:** ~25-30 test cases.

---

### Priority 3: `logging.rs` Redaction Edge Cases

**Problem:** The redaction system is security-sensitive (prevents PII leaks in logs) but testing is thin. Only one test verifies a single pattern against a single input.

**Specific tests to add:**

- **Multiple redaction patterns applied in sequence:** Verify that when two patterns both match, both replacements occur
- **Overlapping patterns:** What happens if pattern A's replacement text matches pattern B?
- **Empty input / whitespace-only input:** Session ID prepending should handle these correctly (current code skips whitespace-only lines)
- **Unicode content:** Redaction patterns should work correctly with multi-byte characters
- **Invalid regex in config:** `init_logging` currently prints a warning and skips - verify the remaining patterns still work
- **Session ID disabled:** Verify no `[session_id]` prefix when `show_session_id` is false
- **Large input:** Verify no performance degradation with many patterns on large log lines

**Estimated scope:** ~10-12 test cases.

---

### Priority 4: `global_config.rs` Error Paths

**Problem:** The happy path is well tested, but error handling and edge cases are not.

**Specific tests to add:**

- **Missing `global_config.yaml`:** Verify `load_config` returns a clear error when the mandatory config file doesn't exist
- **Malformed YAML:** Invalid YAML syntax should produce a descriptive error, not a panic
- **Missing required fields:** YAML with a missing required field (e.g., no `model_name`) should fail with a deserialization error
- **API key accessor functions:** Verify each accessor (`openai_api_key()`, `anthropic_api_key()`, etc.) returns `None` when unset and `Some(&str)` when set
- **`features` HashMap:** Test that feature flags load correctly and default to empty when not specified
- **Multiple environment variable overrides at once:** Set several `APP__*` vars and verify they all apply
- **`get_config_storage` thread safety:** Spawn multiple threads calling `get_config()` concurrently and verify no panics or data races

**Estimated scope:** ~10-15 test cases.

---

### Priority 5: `lib.rs` Tauri Command Tests

**Problem:** The `greet` and `get_app_config` commands have zero tests. While simple, they are the public API surface between frontend and backend.

**Specific tests to add:**

- **`greet` function:**
  - Returns formatted string with the provided name
  - Handles empty string input
  - Handles special characters in name (unicode, HTML entities)

- **`get_app_config` function:**
  - Returns a valid `FrontendConfig` reference
  - Does not contain API keys (defense-in-depth with the sanitization test in `global_config.rs`)

**Estimated scope:** ~5 test cases.

---

## Summary

| Priority | Area | Current Tests | Proposed Tests | Impact |
|----------|------|--------------|----------------|--------|
| P1 | Frontend (Vitest setup + hooks/components) | 0 | ~15-20 | High - zero coverage today |
| P2 | `asset_gen.rs` image processing | 2 | ~25-30 | High - complex logic, minimal coverage |
| P3 | `logging.rs` redaction edge cases | 22 | ~5-8 remaining | Medium - security-sensitive |
| P4 | `global_config.rs` error paths | 8 | ~10-15 | Medium - config is foundational |
| P5 | `lib.rs` Tauri commands | 0 | ~5 | Low - simple functions |

**Total current:** 32 tests
**Total proposed additions:** ~65-82 tests

### What is NOT worth testing

- `main.rs` - trivial wrapper, single function call
- CSS styles - no logic, visual regression testing would be overkill for a template
- CLI argument parsing in `asset_gen.rs` - clap handles this, testing their library is not our job
- `GeminiClient::send_request` - requires mocking HTTP; better covered by integration tests against a local mock server if the project grows
