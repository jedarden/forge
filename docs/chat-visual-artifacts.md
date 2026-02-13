# Forge Chat View Visual Artifacts Documentation

## Date: 2026-02-13
## Bead: fg-2pdq

## Summary

The Chat view in FORGE exhibits severe visual corruption when rendered. The artifacts include overlapping panel content, text fragmentation, and rendering of content from other views bleeding into the Chat view area.

## Reproduction Steps

1. Build and run FORGE: `cargo build --release && ./target/release/forge`
2. Terminal dimensions: 120x40 (Wide mode)
3. Press `:` to switch to Chat view
4. Observe visual corruption

## Visual Artifacts Observed

### 1. Panel Overlap/Content Bleeding

**Description**: Content from other panels (Worker Pool, Task Queue, Subscriptions, Activity Log, Quick Actions) appears to bleed into the Chat view area.

**Evidence from captured output**:
```
│                Chat                                            ││                                                                ││                        2026-02-13 16:15:192026-02-13T16:14:57.93274
3Z  INFO forge_tui::data: Health check: 74 unhealthy, 1 degraded workers detected
```

The Chat panel title appears but log messages and subscription data are interleaved with the Chat content.

### 2. Text Fragmentation/Misalignment

**Description**: Text from different UI elements is fragmented and misaligned across the terminal.

**Evidence**:
```
│ Chat Hist ry                                                   ──────────────  2026-02-13T16:11:25.394827Z  INFO forge_tui::data: Processing subscription: anthropic max_5x (usage: 12500000/Some(45000
000)) commands or ask questions. Examples:
```

- "Chat History" title appears as "Chat Hist ry" (fragmented)
- Timestamps and log messages overlay the Chat panel content
- Subscription usage data appears mid-screen

### 3. Border Corruption

**Description**: Panel borders are incomplete or corrupted.

**Evidence**:
```
┌       ipti  s       ─ ─    ────────────────────────────────────   Activity Log ──────────────────────────────────────────────────
```

The left border and title of what appears to be the Subscriptions panel shows "ipti s" instead of "Subscriptions".

### 4. Multiple Panel Titles Visible

**Description**: Multiple panel titles from the Overview view remain visible in Chat view.

**Evidence**:
- "Chat History" visible but corrupted
- "Activity Log" visible
- "Quick Actions" visible
- "Subscriptions" partially visible

### 5. Input Field Overlap

**Description**: The Chat input field at the bottom overlaps with other content.

**Evidence**:
```
┌ Input ────────────────────────────────────────────────────────────────────────────┌──────────────────────────────┐────────────────────────────────────────────────────────────────────────────────────┐
│> █                                                                                │ Chat backend not initialized │
```

The Input panel shows correct content but is overlapped by another panel border.

## Root Cause Analysis

### Suspected Issues

1. **Buffer Not Being Cleared**: When switching from Overview view to Chat view, the terminal buffer may not be properly cleared, leaving remnants of the previous view.

2. **Layout Calculation Bug**: The Chat view uses a simple 2-panel vertical layout:
   ```rust
   let chunks = Layout::default()
       .direction(Direction::Vertical)
       .constraints([Constraint::Min(5), Constraint::Length(3)])
       .split(area);
   ```
   This should occupy the full content area, but something is causing previous panel content to remain.

3. **No Clear Widget Before Chat Content**: Unlike overlay dialogs which use `frame.render_widget(Clear, area)` before drawing, the Chat view doesn't explicitly clear its area before rendering.

### Code Location

The issue is in `crates/forge-tui/src/app.rs`:
- `draw_chat()` function at line 2340
- `draw_content()` function at line 1959

### Comparison with Working Overlays

Overlays (help, kill dialog, task detail) use:
```rust
// Clear background
frame.render_widget(Clear, overlay_area);
```

The Chat view does NOT use this pattern, which may be the root cause.

## Test Dimensions

| Dimension | Size | Artifact Severity |
|-----------|------|-------------------|
| Narrow | 80x24 | Severe (content bleeding visible) |
| Wide | 120x40 | Severe |
| Wide | 140x45 | Severe (confirmed on 2026-02-13) |
| UltraWide | 160x50 | Severe (confirmed on 2026-02-13) |

### Test Results from 2026-02-13

Testing confirmed visual artifacts persist across all layout modes:

1. **80x24 (Narrow)**: Content bleeding and fragmented text visible
2. **120x40 (Wide)**: Severe overlap between panels, Chat History title corrupted
3. **140x45 (Wide)**: Multiple panel titles from Overview visible in Chat view
4. **160x50 (UltraWide)**: Border corruption, input field overlap

## Screenshots/Captures

Saved to: `/tmp/forge-chat-view-artifacts.txt`

## Recommended Fix

Add `Clear` widget rendering before drawing Chat content:

```rust
fn draw_chat(&self, frame: &mut Frame, area: Rect) {
    // Clear any previous content
    frame.render_widget(Clear, area);

    let theme = self.theme_manager.current();
    let chunks = Layout::default()
    // ... rest of the function
}
```

## Status

- [x] Artifacts reproduced
- [x] Root cause identified (suspected missing Clear widget)
- [x] Tested at multiple dimensions (80x24, 120x40, 140x45, 160x50)
- [ ] Fix implemented
- [ ] Fix tested at multiple dimensions

## Additional Notes

- Chat backend not initialized message appears in Input panel (expected - requires config.yaml with chat_backend section)
- Test script available at `tests/test-forge-chat.sh` for automated testing
- The issue affects all views that don't use the `Clear` widget pattern before rendering
