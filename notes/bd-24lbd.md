# Bead bd-24lbd: Fix and_then_tool_call to work with Arc<Mutex> pattern

## Status: Fix Already Applied

The fix for `and_then_tool_call` was already applied in commit `8255412` on 2026-05-04.

## What Was Fixed

### Before
The test `test_mock_provider_with_tool_calls` was skipped with a TODO comment:
```rust
// For now, skip this test as and_then_tool_call needs fixing
// TODO: Fix and_then_tool_call to work with the Arc<Mutex> pattern
```

### After (Commit 8255412)
The test was properly implemented to verify:
1. `and_then_tool_call` builder method chains a tool call response
2. Multiple responses are returned in correct sequence
3. Tool call parameters are preserved correctly

### Implementation Details

The `MockProvider` already had two patterns for adding tool call responses:

1. **Builder pattern** (`and_then_tool_call`): For initial setup before sharing
   - Takes `mut self`
   - Creates new Arc with updated responses
   - Works when provider is not yet shared

2. **Shared pattern** (`add_tool_call_response`): For dynamic updates after sharing
   - Takes `&self`
   - Locks mutex and modifies in-place
   - Works when provider is shared via `Arc<Mutex<>>`

The fix clarified that both patterns work correctly:
- Use `and_then_tool_call` for initial provider setup
- Use `add_tool_call_response` for dynamic updates on shared providers

## Tests Passing

All related tests pass:
- `test_mock_provider_with_tool_calls` - Builder pattern
- `test_mock_provider_add_tool_call_response_on_shared` - Shared pattern
- `test_mock_provider_dynamic_tool_call_chaining` - Multi-turn conversation pattern

## Related Commits

- `8255412` - test(forge-chat): fix test_mock_provider_with_tool_calls to properly test and_then_tool_call
- `6b76d40` - docs: clarify and_then_* vs add_* methods in MockProvider
