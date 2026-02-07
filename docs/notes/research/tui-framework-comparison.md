# TUI Framework Comparison for Control Panel Dashboard

**Research Date:** 2026-02-07
**Bead ID:** po-7jb

## Executive Summary

This document compares five Python TUI frameworks for building an interactive control panel dashboard with real-time monitoring capabilities.

**Recommendation:** **Textual** is the clear winner for the control panel dashboard due to its modern async architecture, comprehensive widget library, excellent real-time update capabilities, and strong community support.

---

## Comparison Matrix

| Feature | Textual | Rich | urwid | py-cui | asciimatics |
|---------|---------|------|-------|--------|-------------|
| **Real-time Updates** | | | | | |
| Async event loop | ✅ Native | ⚠️ Limited | ⚠️ Requires external | ❌ Blocking | ✅ Async support |
| Live data refresh | ✅ Built-in `set_interval` | ✅ `Live` display | ⚠️ Manual redraw | ❌ Manual | ✅ Scene refresh |
| Widget state updates | ✅ Reactive | ⚠️ Re-render needed | ⚠️ Manual | ⚠️ Manual | ✅ Model/View |
| **Widgets** | | | | | |
| Data tables | ✅ `DataTable` widget | ✅ `Table` renderable | ⚠️ Basic | ⚠️ Basic | ⚠️ Basic |
| Charts/graphs | ✅ Custom + extensions | ❌ None | ❌ None | ❌ None | ❌ None |
| Progress bars | ✅ `ProgressBar`, `Gauge` | ✅ `Progress` | ⚠️ Custom | ⚠️ Custom | ⚠️ Custom |
| Log viewers | ✅ `RichLog`, `Log` widget | ✅ `Console` log handler | ⚠️ Custom | ⚠️ Custom | ⚠️ Custom |
| Text areas | ✅ `TextArea`, `Input` | ❌ None | ✅ `Edit` | ✅ `TextBox` | ✅ `Text` |
| Trees/lists | ✅ `TreeView`, `ListView` | ✅ `Tree` | ⚠️ Custom | ✅ `ScrollMenu` | ⚠️ Custom |
| **Event Handling** | | | | | |
| Keyboard input | ✅ Comprehensive | ❌ None | ✅ Curses-based | ✅ Key bindings | ✅ Screen input |
| Mouse support | ✅ Full | ❌ None | ✅ Basic | ⚠️ Limited | ✅ Full |
| Custom events | ✅ Message system | ❌ None | ⚠️ Signals | ✅ Callbacks | ✅ Effects |
| **Performance** | | | | | |
| Multi-worker updates | ✅ Thread-safe | ⚠️ Not thread-safe | ⚠️ GIL issues | ⚠️ Blocking | ✅ Async |
| Update frequency | ✅ 60 FPS capable | ⚠️ ~10 FPS | ⚠️ Manual | ❌ Slow | ⚠️ Moderate |
| Memory efficiency | ✅ Low | ✅ Low | ✅ Low | ✅ Low | ⚠️ Higher |
| **Documentation** | | | | | |
| Official docs | ✅ Excellent | ✅ Excellent | ⚠️ Basic | ⚠️ Basic | ⚠️ Moderate |
| Examples | ✅ Extensive gallery | ✅ Many | ⚠️ Limited | ⚠️ Few | ⚠️ Some |
| Community | ✅ Large (48k+ stars) | ✅ Large (47k+ stars) | ⚠️ Small | ⚠️ Very small | ⚠️ Small |
| Active development | ✅ Very active | ✅ Active | ⚠️ Slow | ❌ Stagnant | ⚠️ Slow |
| **Integration** | | | | | |
| Python version | ✅ 3.8+ | ✅ 3.7+ | ✅ 3.7+ | ✅ 3.6+ | ✅ 3.6+ |
| Cross-platform | ✅ Yes | ✅ Yes | ✅ Yes (Unix) | ✅ Yes (curses) | ✅ Yes |
| Browser support | ✅ Yes (`textual-web`) | ❌ No | ❌ No | ❌ No | ❌ No |
| Testing framework | ✅ Built-in pytest plugin | ❌ None | ❌ None | ❌ None | ❌ None |

---

## Detailed Framework Analysis

### 1. Textual (⭐ Recommended)

**Website:** https://textualize.io/textual
**GitHub:** https://github.com/Textualize/textual
**PyPI:** `textual`

#### Strengths for Control Panel Dashboard

1. **Real-time Data Excellence**
   - Native async/await support for non-blocking updates
   - Built-in `set_interval()` for periodic worker status polling
   - Reactive widget system - data changes propagate automatically
   - Thread-safe message passing for multi-worker coordination

2. **Widget Library**
   - `DataTable`: Sortable, filterable tables for pool/worker status
   - `ProgressBar` & `Gauge`: Visual optimization progress
   - `RichLog`: Scrolling log viewer with color/level filtering
   - `TreeView`: Hierarchical workspace/pool display
   - `Header`, `Footer`: Status bar with keybindings
   - `Tabs`: Multiple workspace monitoring

3. **Multi-Worker Support**
   - Worker-safe `app.call_from_thread()` for updates from background threads
   - WebSocket support for remote worker monitoring
   - `textual-web` allows serving TUI in browser

4. **Developer Experience**
   - CSS-like styling system
   - Dev console for debugging (`textual-dev`)
   - Built-in testing with pytest integration
   - Hot reload during development

5. **Active Community**
   - 48k+ GitHub stars, rapid release cycle
   - Active Discord community
   - Extensive example gallery
   - Third-party extensions (charting, databases)

#### Weaknesses
- Steeper learning curve than simpler libraries
- More dependencies (async lifecycle management)
- CSS styling can be complex for beginners

#### Code Example - Real-time Dashboard

```python
from textual.app import App, ComposeResult
from textual.widgets import DataTable, Header, Footer, ProgressBar, RichLog
from textual.containers import Horizontal, Vertical
import asyncio

class PoolOptimizerDashboard(App):
    CSS = """
    Screen {
        layout: vertical;
    }
    #pools {
        height: 1fr;
    }
    #logs {
        height: 1fr;
    }
    """

    def compose(self) -> ComposeResult:
        yield Header()
        with Horizontal():
            yield DataTable(id="pools")
            with Vertical():
                yield ProgressBar(id="progress")
                yield RichLog(id="logs")
        yield Footer()

    def on_ready(self) -> None:
        # Initialize worker status table
        table = self.query_one(DataTable)
        table.add_columns("Worker", "Pool", "Status", "Progress", "ETA")

        # Start polling workers every second
        self.set_interval(1, self.update_worker_status)

    async def update_worker_status(self):
        """Update worker status from shared state."""
        async with worker_state_lock:
            for worker_id, state in worker_states.items():
                # Update table rows
                # Update progress bars
                # Append to logs
                pass
```

---

### 2. Rich

**Website:** https://rich.readthedocs.io/
**GitHub:** https://github.com/Textualize/rich
**PyPI:** `rich`

#### Strengths
- Excellent formatting library for terminal output
- `Live` display for auto-refreshing content
- `Progress` with multiple bars
- Beautiful tables with borders/colors
- Great for simple dashboards

#### Weaknesses for Control Panel
- **Not a full TUI framework** - no interactive widgets
- No keyboard input handling
- No state management
- `Live` display requires full re-render on updates
- Not ideal for complex, interactive dashboards

#### When to Use
- Simple status displays
- Progress bars for batch operations
- Beautiful CLI output (not interactive)

---

### 3. urwid

**Website:** https://urwid.org/
**GitHub:** https://github.com/urwid/urwid
**PyPI:** `urwid`

#### Strengths
- Mature, stable curses-based library
- Low-level control over terminal
- Good for traditional console apps
- Works with various event loops (Tornado, Twisted, Trio, asyncio)

#### Weaknesses for Control Panel
- **No built-in widgets for modern UI** (tables, progress bars require custom code)
- Manual redraw management
- Older API, less Pythonic
- Limited documentation and examples
- Small community, slow development
- No async-first design

#### When to Use
- Need low-level terminal control
- Building on legacy urwid codebase
- Require non-standard event loop integration

---

### 4. py-cui

**GitHub:** https://github.com/jwlodek/py_cui
**PyPI:** `py-cui`

#### Strengths
- Simple, intuitive grid-based layout
- Good widgets for basic needs
- Easy to learn (Tkinter-like)
- Cross-platform (via windows-curses)

#### Weaknesses for Control Panel
- **Stagnant development** (last release 2021)
- Very small community
- No async support (blocking event loop)
- Poor performance with frequent updates
- Limited real-time capabilities
- No built-in data tables or charts

#### When to Use
- Simple, static forms or menus
- Quick prototypes
- Learning TUI basics

---

### 5. asciimatics

**Website:** https://asciimatics.readthedocs.io/
**GitHub:** https://github.com/peterbrittain/asciimatics
**PyPI:** `asciimatics`

#### Strengths
- Cross-platform (Windows, Linux, macOS)
- Excellent for ASCII animations
- Good widget set
- Async framework support

#### Weaknesses for Control Panel
- **Animation-focused** (not dashboard-optimized)
- Complex scene/effect model for simple UIs
- Smaller community
- Less documentation than Textual
- No built-in data visualization
- More suited to demos than functional dashboards

#### When to Use
- ASCII art animations
- Visual effects in terminal
- Cross-platform needs

---

## Decision Matrix Scores

| Criterion (Weight) | Textual | Rich | urwid | py-cui | asciimatics |
|--------------------|---------|------|-------|--------|-------------|
| **Real-time Updates (30%)** | 10 | 5 | 4 | 2 | 6 |
| **Widget Support (25%)** | 10 | 4 | 3 | 4 | 5 |
| **Multi-worker Support (20%)** | 10 | 2 | 3 | 1 | 5 |
| **Documentation (15%)** | 9 | 9 | 5 | 4 | 6 |
| **Community (10%)** | 10 | 9 | 5 | 2 | 4 |
| **Weighted Total** | **9.85** | **5.15** | **3.85** | **2.45** | **5.35** |

---

## Recommendation

### Choose **Textual** for the control panel dashboard

**Justification:**

1. **Real-time Excellence**: Native async support and thread-safe updates make it ideal for monitoring multiple workers simultaneously.

2. **Widget Coverage**: Built-in `DataTable`, `RichLog`, `ProgressBar`, and `Gauge` widgets directly address dashboard needs without custom implementation.

3. **Multi-worker Architecture**: The `app.call_from_thread()` method allows safe updates from background worker processes/threads.

4. **Future-proof**: Active development, large community, and browser support via `textual-web` ensure long-term viability.

5. **Developer Productivity**: CSS styling, dev console, and testing framework accelerate development.

### Implementation Plan

```python
# Recommended architecture for control panel TUI
# /home/coder/research/control-panel/src/tui/dashboard.py

from textual.app import App
from textual.widgets import (
    DataTable, Header, Footer,
    ProgressBar, RichLog, TabbedContent, TabPane
)
from textual.containers import Horizontal, Vertical
import asyncio
from pathlib import Path

class PoolOptimizerTUI(App):
    """Main TUI application for control panel monitoring."""

    TITLE = "Control Panel Dashboard"
    BINDINGS = [
        ("q", "quit", "Quit"),
        ("r", "refresh", "Refresh"),
        ("d", "toggle_dark", "Toggle Dark Mode"),
    ]

    def compose(self):
        """Build the UI layout."""
        yield Header()
        with TabbedContent():
            with TabPane("Workers", id="workers-tab"):
                yield DataTable(id="worker-table")
            with TabPane("Pools", id="pools-tab"):
                yield DataTable(id="pool-table")
            with TabPane("Logs", id="logs-tab"):
                yield RichLog(id="log-viewer")
        with Horizontal():
            yield ProgressBar(id="overall-progress", show_eta=True)
        yield Footer()

    async def on_mount(self):
        """Initialize the dashboard."""
        # Set up tables
        self._setup_worker_table()
        self._setup_pool_table()

        # Start update loop
        self.set_interval(1.0, self.update_dashboard)

    async def update_dashboard(self):
        """Poll worker states and update UI."""
        # Read from shared state/Redis/files
        # Update widgets
        pass
```

### Dependencies

```txt
# requirements.txt for TUI
textual>=0.80.0
textual-dev>=0.1.0
rich>=13.0.0  # For log formatting
aiohttp>=3.9.0  # For async HTTP to workers
```

---

## Alternative Options

### If Textual is Not Available

1. **For simple dashboards**: Use `rich.Live` with custom update logic
2. **For legacy systems**: Consider `urwid` with async event loop
3. **For quick prototypes**: Use `py-cui` for basic UI needs

### Hybrid Approach

Use Rich for beautiful CLI output + Textual for the interactive dashboard:

```python
# Use Rich for startup/logs
from rich.console import Console
console = Console()
console.print("[bold green]Starting Control Panel...[/bold green]")

# Launch Textual TUI
from dashboard import PoolOptimizerTUI
app = PoolOptimizerTUI()
app.run()
```

---

## Conclusion

Textual is the optimal choice for the control panel dashboard due to its:
- Superior real-time update capabilities
- Comprehensive widget library
- Multi-worker support architecture
- Active community and documentation
- Modern, maintainable codebase

The framework's async-first design aligns perfectly with monitoring concurrent worker processes, and its built-in widgets eliminate the need for custom implementation of common dashboard components.

---

## References

- Textual GitHub: https://github.com/Textualize/textual
- Textual Documentation: https://textualize.io/textual
- Rich Documentation: https://rich.readthedocs.io/
- Real Python Textual Guide: https://realpython.com/python-textual/
- Textual Discord: https://discord.gg/Textual
