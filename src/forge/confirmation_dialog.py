"""
FORGE Confirmation Dialog Widget

A Textual widget for confirming tool execution requests.
Displays tool name, arguments, and preview of changes.
"""

from dataclasses import dataclass
from typing import Any, Callable

from textual.widget import Widget
from textual.widgets import Button, Static
from textual.containers import Container, Horizontal, Vertical
from textual.reactive import reactive
from textual import on
from rich.text import Text
from rich.panel import Panel


# =============================================================================
# Confirmation Dialog Data
# =============================================================================


@dataclass
class ConfirmationRequest:
    """A request for user confirmation."""
    tool_name: str
    arguments: dict[str, Any]
    message: str
    preview_data: dict[str, Any] | None = None


@dataclass
class ConfirmationResponse:
    """User's response to confirmation request."""
    approved: bool
    excluded_tools: list[tuple[str, dict[str, Any]]] | None = None  # For batch operations


# =============================================================================
# Confirmation Dialog Widget
# =============================================================================


class ToolConfirmationDialog(Widget):
    """
    Confirmation dialog for tool execution.

    Shows:
    - Tool name and description
    - Arguments being passed
    - Preview of changes (if applicable)
    - Confirm/Cancel/Exclude buttons
    """

    DEFAULT_CSS = """
    ToolConfirmationDialog {
        layer: overlay;
        dock: top;
        height: 30;
        padding: 1 2;
    }

    .confirmation-container {
        background: $surface;
        border: thick $primary;
        border-subtitle: "Confirm Tool Execution";
        padding: 1;
        height: 100%;
    }

    .confirmation-title {
        text-style: bold;
        margin: 0 1;
        text-align: center;
    }

    .confirmation-message {
        margin: 1 1;
        text-style: italic;
        color: $warning;
    }

    .confirmation-arguments {
        margin: 1 1;
        padding: 1;
        background: $panel;
        border: round $primary;
    }

    .confirmation-preview {
        margin: 1 1;
        padding: 1;
        background: $panel;
        border: round $accent;
        height: 10;
    }

    .confirmation-buttons {
        dock: bottom;
        height: 3;
        align: center_middle;
    }

    Button {
        margin: 0 1;
    }

    .confirm-yes {
        background: $success;
    }

    .confirm-no {
        background: $error;
    }

    .confirm-exclude {
        background: $warning;
    }
    """

    # Reactive state
    showing: reactive[bool] = False
    request: reactive[ConfirmationRequest | None] = reactive(None)
    response: ConfirmationResponse | None = None
    _callback: Callable[[ConfirmationResponse], None] | None = None

    def __init__(self, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        self._setup_ui()

    def _setup_ui(self) -> None:
        """Setup the UI components."""
        self.container = Container(classes="confirmation-container")

        # Title
        self.title = Static("", classes="confirmation-title")

        # Message
        self.message = Static("", classes="confirmation-message")

        # Arguments display
        self.arguments_display = Static("", classes="confirmation-arguments")

        # Preview display
        self.preview_display = Static("", classes="confirmation-preview")

        # Buttons
        self.button_container = Horizontal(classes="confirmation-buttons")
        self.yes_button = Button("Confirm (Y)", variant="success", classes="confirm-yes")
        self.no_button = Button("Cancel (N)", variant="error", classes="confirm-no")
        self.exclude_button = Button("Exclude (E)", variant="warning", classes="confirm-exclude", disabled=True)

        self.button_container.mount(self.yes_button, self.no_button, self.exclude_button)

        # Mount everything
        self.container.mount(
            self.title,
            self.message,
            self.arguments_display,
            self.preview_display,
            self.button_container
        )

        self.mount(self.container)

    def show(
        self,
        request: ConfirmationRequest,
        callback: Callable[[ConfirmationResponse], None]
    ) -> None:
        """
        Show the confirmation dialog.

        Args:
            request: The confirmation request to show
            callback: Function to call with user's response
        """
        self.request = request
        self._callback = callback
        self.showing = True
        self.visible = True

        # Update UI
        self._update_display()

        # Focus yes button
        self.yes_button.focus()

    def hide(self) -> None:
        """Hide the confirmation dialog."""
        self.showing = False
        self.visible = False
        self.request = None
        self.response = None
        self._callback = None

    def _update_display(self) -> None:
        """Update the display based on current request."""
        if self.request is None:
            return

        req = self.request

        # Update title
        self.title.update(f"Execute Tool: {req.tool_name}")

        # Update message
        self.message.update(req.message)

        # Update arguments
        args_text = self._format_arguments(req.arguments)
        self.arguments_display.update(args_text)

        # Update preview if available
        if req.preview_data:
            preview_text = self._format_preview(req.preview_data)
            self.preview_display.update(preview_text)
            self.preview_display.visible = True
        else:
            self.preview_display.visible = False

        # Enable exclude button for batch operations
        if "count" in req.arguments and req.arguments.get("count", 1) > 1:
            self.exclude_button.disabled = False
        else:
            self.exclude_button.disabled = True

    def _format_arguments(self, arguments: dict[str, Any]) -> str:
        """Format arguments for display."""
        lines = ["[bold]Arguments:[/bold]"]
        for key, value in arguments.items():
            if isinstance(value, (list, dict)):
                value_str = str(value)[:100]  # Truncate long values
            else:
                value_str = str(value)
            lines.append(f"  {key}: {value_str}")
        return "\n".join(lines)

    def _format_preview(self, preview_data: dict[str, Any]) -> str:
        """Format preview data for display."""
        lines = ["[bold]Preview:[/bold]"]
        for key, value in preview_data.items():
            if isinstance(value, list):
                lines.append(f"  [bold]{key}:[/bold]")
                for item in value[:10]:  # Show first 10 items
                    lines.append(f"    - {item}")
                if len(value) > 10:
                    lines.append(f"    ... and {len(value) - 10} more")
            else:
                lines.append(f"  {key}: {value}")
        return "\n".join(lines)

    @on(Button.Pressed, ".confirm-yes")
    def on_yes_pressed(self, event: Button.Pressed) -> None:
        """Handle yes button press."""
        self.response = ConfirmationResponse(approved=True)
        if self._callback:
            self._callback(self.response)
        self.hide()

    @on(Button.Pressed, ".confirm-no")
    def on_no_pressed(self, event: Button.Pressed) -> None:
        """Handle no button press."""
        self.response = ConfirmationResponse(approved=False)
        if self._callback:
            self._callback(self.response)
        self.hide()

    @on(Button.Pressed, ".confirm-exclude")
    def on_exclude_pressed(self, event: Button.Pressed) -> None:
        """Handle exclude button press."""
        # For now, just reject - exclusion logic would go here
        self.response = ConfirmationResponse(approved=False)
        if self._callback:
            self._callback(self.response)
        self.hide()

    def on_key(self, event) -> None:
        """Handle keyboard shortcuts."""
        if not self.showing:
            return

        key = event.key
        if key == "y" or key == "Y":
            self.yes_button.press()
        elif key == "n" or key == "N" or key == "escape":
            self.no_button.press()
        elif key == "e" or key == "E":
            if not self.exclude_button.disabled:
                self.exclude_button.press()


# =============================================================================
# Simple Confirmation Screen (alternative overlay)
# =============================================================================


class ConfirmationScreen(Widget):
    """
    Full-screen confirmation overlay.

    Alternative to ToolConfirmationDialog that takes up the entire screen.
    Useful for more complex confirmations with multiple tools.
    """

    DEFAULT_CSS = """
    ConfirmationScreen {
        layer: overlay;
        dock: fill;
    }

    .screen-container {
        background: $surface;
        padding: 2 4;
        height: 100%;
    }

    .screen-title {
        text-style: bold;
        text-align: center;
        margin: 0 1;
    }

    .screen-message {
        margin: 1 1;
        text-style: italic;
        color: $warning;
    }

    .screen-content {
        margin: 1 1;
        padding: 1;
        background: $panel;
        border: round $primary;
        height: 1fr;
    }

    .screen-footer {
        dock: bottom;
        height: 3;
        padding: 1;
        text-align: center;
        text-style: dim;
    }
    """

    showing: reactive[bool] = False
    requests: reactive[list[ConfirmationRequest]] = reactive(lambda: [])
    responses: list[ConfirmationResponse] = []
    _callback: Callable[[list[ConfirmationResponse]], None] | None = None

    def __init__(self, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        self._setup_ui()

    def _setup_ui(self) -> None:
        """Setup UI components."""
        self.container = Vertical(classes="screen-container")

        self.title = Static("Confirm Tool Execution", classes="screen-title")
        self.message = Static("", classes="screen-message")
        self.content = Static("", classes="screen-content")
        self.footer = Static(
            "[Y] Confirm All  [N] Cancel All  [E] Exclude Some  [Esc] Cancel",
            classes="screen-footer"
        )

        self.container.mount(self.title, self.message, self.content, self.footer)
        self.mount(self.container)

    def show_batch(
        self,
        requests: list[ConfirmationRequest],
        callback: Callable[[list[ConfirmationResponse]], None]
    ) -> None:
        """Show confirmation for batch tool execution."""
        self.requests = requests
        self._callback = callback
        self.showing = True
        self.visible = True
        self.responses = []

        # Update UI
        self._update_display()

    def _update_display(self) -> None:
        """Update display based on current requests."""
        if not self.requests:
            return

        # Update message
        count = len(self.requests)
        self.message.update(
            f"{count} tool execution(s) pending confirmation"
        )

        # Update content
        lines = ["[bold]Tools to Execute:[/bold]\n"]
        for i, req in enumerate(self.requests):
            lines.append(f"{i + 1}. [bold]{req.tool_name}[/bold]")
            for key, value in req.arguments.items():
                lines.append(f"   {key}: {value}")
            lines.append("")

        self.content.update("\n".join(lines))

    def confirm_all(self) -> None:
        """Confirm all pending requests."""
        self.responses = [
            ConfirmationResponse(approved=True)
            for _ in self.requests
        ]
        if self._callback:
            self._callback(self.responses)
        self.hide()

    def cancel_all(self) -> None:
        """Cancel all pending requests."""
        self.responses = [
            ConfirmationResponse(approved=False)
            for _ in self.requests
        ]
        if self._callback:
            self._callback(self.responses)
        self.hide()

    def hide(self) -> None:
        """Hide the confirmation screen."""
        self.showing = False
        self.visible = False
        self.requests = []
        self.responses = []
        self._callback = None

    def on_key(self, event) -> None:
        """Handle keyboard shortcuts."""
        if not self.showing:
            return

        key = event.key
        if key == "y" or key == "Y":
            self.confirm_all()
        elif key == "n" or key == "N" or key == "escape":
            self.cancel_all()
