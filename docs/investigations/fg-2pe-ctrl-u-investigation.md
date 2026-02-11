# Investigation Report: Ctrl+U Keystroke Handling in Forge TUI

**Bead:** fg-2pe
**Date:** 2026-02-11
**Status:** Complete

## Summary

Ctrl+U **IS working correctly** - the keystroke reaches the application and triggers the update process. The perceived "lack of visual feedback" is actually caused by a **different issue**: the binary copy fails with "Text file busy" error, but this error message gets lost in the TUI output.

## Investigation Findings

### 1. Keystroke Handling (Working Correctly)

**Code path:**
- `crates/forge-tui/src/event.rs:144-146` - Ctrl+U maps to `AppEvent::Update`
- `crates/forge-tui/src/app.rs:715-717` - `AppEvent::Update` calls `trigger_update()`
- `crates/forge-tui/src/app.rs:759-808` - `trigger_update()` runs cargo build and attempts copy

**Evidence from tmux capture:**
```
Finished `release` profile [optimized] target(s) in 0.25s
cp: cannot create regular file '/home/coder/.cargo/bin/forge': Text file busy
```

### 2. Tmux Configuration (Not Interfering)

The only Ctrl+U binding in tmux is for `copy-mode-vi` (halfpage-up), which only activates when in copy mode. Normal mode passes Ctrl+U through to the application.

### 3. Terminal Line-Kill (Not an Issue)

The standard `\C-u: unix-line-discard` binding is irrelevant because:
- forge-tui uses crossterm with `enable_raw_mode()`
- Raw mode disables line editing and passes all keystrokes directly to the application

### 4. Root Cause: Binary Copy Fails

The update process:
1. ✅ Status message set: "Rebuilding forge..."
2. ✅ Cargo build runs and succeeds
3. ❌ `cp target/release/forge ~/.cargo/bin/forge` fails with "Text file busy"
4. ⚠️ Error message shown but may not be visible (TUI rendering issue)

**"Text file busy"** occurs because Linux prevents overwriting an executable file while it's running.

## Visual Feedback Issues

The actual problems with visual feedback are:

1. **Error message visibility**: The `cp` error appears in the TUI but gets rendered in an awkward location (blends with UI elements)
2. **No persistent indication**: After the error, the status reverts but there's no clear indication the update failed
3. **Blocking operation**: `child.wait()` blocks the TUI, preventing immediate visual feedback during the build

## Recommended Fixes for fg-3cm

### Short-term (Visual Feedback)
1. **Add a dedicated "Update Status" widget** or modal that shows:
   - "Building..." during cargo build
   - "Installing..." during copy
   - Success or failure result with clear error messages

2. **Non-blocking execution**: Run the update in a separate thread with status polling

### Long-term (Binary Replacement)
1. **Use rename/move instead of copy**:
   - Build to a temp location
   - Rename/move the binary (atomic operation)
   - This may still fail but is more robust

2. **Exit-and-restart pattern**:
   - Exit forge gracefully
   - Have a wrapper script that runs the update then restarts forge
   - This is the most reliable approach for self-updating binaries

3. **Update indicator**: Show "Update ready - restart required" instead of trying to overwrite in-place

## Artifacts

- Key event handler: `crates/forge-tui/src/event.rs:144-147`
- Update trigger: `crates/forge-tui/src/app.rs:759-808`
- Update script: `update-forge.sh`

## Conclusion

**The investigation reveals that Ctrl+U is working correctly.** The issue reported as "lack of visual feedback" is actually a combination of:
1. The update process failing silently (binary overwrite blocked)
2. Error messages not being clearly displayed in the TUI
3. No dedicated UI component for update status

The fix in fg-3cm should focus on better status display and handling the "Text file busy" scenario gracefully.
