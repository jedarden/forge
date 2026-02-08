"""
FORGE Error Display Patterns

Implements error display patterns from ADR 0014:
1. Transient Errors (Non-Blocking) - Notification overlay
2. Component Errors (Degrade Component) - In-panel error display
3. Fatal Errors (Block Startup) - Full-screen blocking error
4. User Action Required - Dialog with actionable buttons

Core Principles (per ADR 0014):
- Visibility First: Show errors clearly in TUI
- No Silent Failures: Every error is visible to user
- No Automatic Retry: User decides if/when to retry
- Degrade Gracefully: Broken component doesn't crash entire app
- Clear Error Messages: Actionable guidance, not technical jargon
"""

from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Callable
from textual.widget import Widget
from textual.widgets import Button, Static
from textual.containers import Container, Horizontal, Vertical
from textual.reactive import reactive
from textual import on


# =============================================================================
# Error Types and Categories
# =============================================================================


class ErrorSeverity(Enum):
    """Error severity levels per ADR 0014"""
    WARNING = "warning"  # Yellow: Degraded but functional
    ERROR = "error"      # Red: Component failed
    FATAL = "fatal"      # Red: Cannot continue


@dataclass
class ErrorAction:
    """An actionable button for error dialogs"""
    label: str
    callback: Callable[[], Any] | None
    variant: str = "default"  # default, primary, success, warning, error

    def __post_init__(self):
        # Map severity to Textual button variant
        if self.variant == "default":
            if self.callback is None:
                self.variant = "error"  # Dismiss action
            else:
                self.variant = "default"


@dataclass
class ErrorDetails:
    """Detailed error information per ADR 0014"""
    title: str
    message: str  # Primary error message (clear, non-technical)
    severity: ErrorSeverity = ErrorSeverity.ERROR
    context: dict[str, Any] = field(default_factory=dict)  # What component failed
    guidance: list[str] = field(default_factory=list)  # How to fix (3-5 items)
    actions: list[ErrorAction] = field(default_factory=list)  # Quick action buttons


# =============================================================================
# 1. Transient Notification Pattern (Non-Blocking)
# =============================================================================


class NotificationOverlay(Widget):
    """
    Transient notification overlay for non-blocking errors.

    Per ADR 0014:
    - Shows notification, doesn't interrupt workflow
    - Auto-dismisses after timeout or on user action
    - Supports severity levels (info, warning, error, success)

    Example:
        self.notify("⚠️  Cost update delayed (database locked)", severity="warning")
    """

    DEFAULT_CSS = """
    NotificationOverlay {
        layer: overlay;
        dock: top;
        height: 3;
        padding: 0 1;
    }

    .notification-container {
        background: $surface;
        border: thick $primary;
        padding: 0 1;
        height: 100%;
        align: center_top;
    }

    .notification-content {
        display: flex;
        flex-direction: row;
        align: center_middle;
        height: 100%;
    }

    .notification-icon {
        margin: 0 1;
        text-style: bold;
    }

    .notification-message {
        margin: 0 1;
    }

    .notification-dismiss {
        margin: 0 1;
        text-style: dim;
    }

    /* Severity-specific styling */
    .notification-info {
        border-subtitle: "Info";
        background: $panel;
    }

    .notification-warning {
        border-subtitle: "Warning";
        background: $warning 15%;
        text-style: bold;
    }

    .notification-error {
        border-subtitle: "Error";
        background: $error 15%;
        text-style: bold;
    }

    .notification-success {
        border-subtitle: "Success";
        background: $success 15%;
    }
    """

    # Reactive state
    showing: reactive[bool] = False
    message: reactive[str] = reactive("")
    severity: reactive[str] = reactive("info")
    _dismiss_callback: Callable[[], None] | None = None

    def __init__(self, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        self._setup_ui()

    def _setup_ui(self) -> None:
        """Setup the UI components."""
        self.container = Container(classes="notification-container")

        # Content
        self.content = Horizontal(classes="notification-content")
        self.icon = Static("", classes="notification-icon")
        self.message_display = Static("", classes="notification-message")
        self.dismiss_hint = Static("[Press ESC to dismiss]", classes="notification-dismiss")

        self.content.mount(self.icon, self.message_display, self.dismiss_hint)
        self.container.mount(self.content)
        self.mount(self.container)

    def show(
        self,
        message: str,
        severity: str = "info",
        timeout: float | None = None,
        callback: Callable[[], None] | None = None
    ) -> None:
        """
        Show a transient notification.

        Args:
            message: The notification message
            severity: info, warning, error, or success
            timeout: Auto-dismiss timeout in seconds (None = manual dismiss)
            callback: Optional callback when dismissed
        """
        self.message = message
        self.severity = severity
        self._dismiss_callback = callback
        self.showing = True
        self.visible = True

        # Update UI
        self._update_display()

        # Set up auto-dismiss if timeout specified
        if timeout:
            self.set_timer(timeout, self.dismiss)

    def dismiss(self) -> None:
        """Dismiss the notification."""
        if not self.showing:
            return

        # Call callback if provided
        if self._dismiss_callback:
            self._dismiss_callback()

        self.showing = False
        self.visible = False
        self.message = ""
        self.severity = "info"
        self._dismiss_callback = None

    def _update_display(self) -> None:
        """Update the display based on current message."""
        # Update icon based on severity
        icons = {
            "info": "ℹ️",
            "warning": "⚠️",
            "error": "❌",
            "success": "✅"
        }
        self.icon.update(icons.get(self.severity, "ℹ️"))

        # Update message
        self.message_display.update(self.message)

        # Update container class
        self.container.set_class(True, f"notification-{self.severity}")

    def on_key(self, event) -> None:
        """Handle keyboard shortcuts."""
        if not self.showing:
            return

        if event.key == "escape" or event.key == " " or event.key == "enter":
            self.dismiss()


# =============================================================================
# 2. Component Error Pattern (In-Panel Error Display)
# =============================================================================


class ComponentErrorWidget(Static):
    """
    In-panel error display for component errors.

    Per ADR 0014:
    - Shows error in component panel
    - Component degrades gracefully, app keeps running
    - Shows actionable guidance

    Example:
        self.show_component_error(
            component="chat",
            error="Backend unavailable",
            fallback="Using hotkey-only mode"
        )
    """

    DEFAULT_CSS = """
    ComponentErrorWidget {
        margin: 1 0;
        padding: 1;
        background: $error 10%;
        border: round $error;
        display: none;
    }

    ComponentErrorWidget.-showing {
        display: block;
    }

    .error-header {
        text-style: bold;
        color: $error;
        margin: 0 0 1 0;
    }

    .error-message {
        margin: 0 0 1 0;
    }

    .error-fallback {
        margin: 0 0 1 0;
        text-style: italic;
        color: $warning;
    }

    .error-guidance {
        margin: 1 0 0 0;
        padding: 1 0 0 0;
        border-top: solid $error 10%;
    }

    .error-guidance-title {
        text-style: bold;
        margin: 0 0 1 0;
    }

    .error-guidance-item {
        margin: 0 0 0 2;
        text-style: dim;
    }
    """

    component_name: reactive[str] = reactive("")
    error_message: reactive[str] = reactive("")
    fallback_message: reactive[str] = reactive("")
    guidance: reactive[list[str]] = reactive(list)

    def __init__(self, component: str = "", **kwargs: Any) -> None:
        super().__init__(**kwargs)
        self.component_name = component

    def compose(self):
        """Compose the error widget."""
        yield Static("", classes="error-header")
        yield Static("", classes="error-message")
        yield Static("", classes="error-fallback")
        yield Vertical(classes="error-guidance")

    def on_mount(self) -> None:
        """Initialize display on mount."""
        self._update_display()

    def watch_error_message(self, old: str, new: str) -> None:
        """React to error message changes."""
        self._update_display()

    def show_error(
        self,
        error: str,
        fallback: str = "",
        guidance: list[str] | None = None
    ) -> None:
        """
        Show component error.

        Args:
            error: Primary error message
            fallback: Fallback mode description
            guidance: List of actionable guidance steps
        """
        self.error_message = error
        self.fallback_message = fallback
        self.guidance = guidance or []
        self.set_class(True, "-showing")

    def clear_error(self) -> None:
        """Clear the error (component recovered)."""
        self.error_message = ""
        self.fallback_message = ""
        self.guidance = []
        self.set_class(False, "-showing")

    def _update_display(self) -> None:
        """Update the display."""
        # Update header
        header = self.query_one(".error-header", Static)
        if self.error_message:
            header.update(f"⚠️  {self.component_name} Error")
        else:
            header.update("")

        # Update message
        message = self.query_one(".error-message", Static)
        message.update(self.error_message)

        # Update fallback
        fallback = self.query_one(".error-fallback", Static)
        if self.fallback_message:
            fallback.update(f"Fallback: {self.fallback_message}")
        else:
            fallback.update("")

        # Update guidance
        guidance_container = self.query_one(".error-guidance", Vertical)
        guidance_container.remove_children()

        if self.guidance:
            title = Static("Suggested actions:", classes="error-guidance-title")
            guidance_container.mount(title)

            for item in self.guidance:
                guidance_item = Static(f"  • {item}", classes="error-guidance-item")
                guidance_container.mount(guidance_item)


# =============================================================================
# 3. Fatal Error Pattern (Full-Screen Blocking Error)
# =============================================================================


class FatalErrorScreen(Widget):
    """
    Full-screen blocking error for fatal errors.

    Per ADR 0014:
    - Shows full-screen error
    - Blocks app startup/operation
    - User must fix error to continue
    - Shows clear guidance and next steps

    Example:
        self.show_fatal_error(
            title="Cannot Start FORGE",
            errors=["Cannot write to ~/.forge (permission denied)"],
            guidance=["Fix the errors above and restart", ...]
        )
    """

    DEFAULT_CSS = """
    FatalErrorScreen {
        layer: overlay;
        dock: fill;
    }

    .fatal-container {
        background: $surface;
        padding: 4;
        height: 100%;
        align: center_middle;
    }

    .fatal-icon {
        text-align: center;
        text-style: bold;
        margin: 0 0 2 0;
        color: $error;
    }

    .fatal-title {
        text-align: center;
        text-style: bold;
        margin: 0 0 2 0;
        color: $error;
    }

    .fatal-errors {
        margin: 2 0;
        padding: 2;
        background: $error 10%;
        border: round $error;
    }

    .fatal-errors-title {
        text-style: bold;
        margin: 0 0 1 0;
    }

    .fatal-error-item {
        margin: 0 0 0 2;
    }

    .fatal-guidance {
        margin: 2 0;
        padding: 2;
        background: $panel;
        border: round $primary;
    }

    .fatal-guidance-title {
        text-style: bold;
        margin: 0 0 1 0;
    }

    .fatal-guidance-item {
        margin: 0 0 0 2;
    }

    .fatal-footer {
        margin: 2 0 0 0;
        text-align: center;
        text-style: dim;
    }

    .fatal-button {
        margin: 1 1;
    }
    """

    title: reactive[str] = reactive("Fatal Error")
    errors: reactive[list[str]] = reactive(list)
    guidance: reactive[list[str]] = reactive(list)
    exit_on_dismiss: reactive[bool] = True
    _callback: Callable[[], None] | None = None

    def __init__(self, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        self._setup_ui()

    def _setup_ui(self) -> None:
        """Setup the UI components."""
        self.container = Vertical(classes="fatal-container")

        # Icon and title
        self.icon = Static("⚠️", classes="fatal-icon")
        self.title_display = Static("", classes="fatal-title")

        # Errors section
        self.errors_container = Vertical(classes="fatal-errors")
        self.errors_title = Static("Errors:", classes="fatal-errors-title")
        self.errors_list = Vertical()

        self.errors_container.mount(self.errors_title, self.errors_list)

        # Guidance section
        self.guidance_container = Vertical(classes="fatal-guidance")
        self.guidance_title = Static("Fix:", classes="fatal-guidance-title")
        self.guidance_list = Vertical()

        self.guidance_container.mount(self.guidance_title, self.guidance_list)

        # Footer
        self.footer = Static("Press any key to exit", classes="fatal-footer")

        # Mount everything
        self.container.mount(
            self.icon,
            self.title_display,
            self.errors_container,
            self.guidance_container,
            self.footer
        )
        self.mount(self.container)

    def show(
        self,
        title: str,
        errors: list[str],
        guidance: list[str],
        exit_on_dismiss: bool = True,
        callback: Callable[[], None] | None = None
    ) -> None:
        """
        Show fatal error screen.

        Args:
            title: Error title
            errors: List of error messages
            guidance: List of fix suggestions
            exit_on_dismiss: Whether to exit app on dismiss
            callback: Optional callback before exit
        """
        self.title = title
        self.errors = errors
        self.guidance = guidance
        self.exit_on_dismiss = exit_on_dismiss
        self._callback = callback
        self.visible = True

        self._update_display()

    def _update_display(self) -> None:
        """Update the display."""
        # Update title
        self.title_display.update(self.title)

        # Update errors
        self.errors_list.remove_children()
        for error in self.errors:
            error_item = Static(f"  • {error}", classes="fatal-error-item")
            self.errors_list.mount(error_item)

        # Update guidance
        self.guidance_list.remove_children()
        for item in self.guidance:
            guidance_item = Static(f"  • {item}", classes="fatal-guidance-item")
            self.guidance_list.mount(guidance_item)

        # Update footer
        if self.exit_on_dismiss:
            self.footer.update("Press any key to exit")
        else:
            self.footer.update("Press ESC to dismiss")

    def on_key(self, event) -> None:
        """Handle keyboard input."""
        if not self.visible:
            return

        # Call callback if provided
        if self._callback:
            self._callback()

        if self.exit_on_dismiss:
            # Exit the app
            from textual.app import App
            app = self.app
            if isinstance(app, App):
                app.exit(1)
        else:
            self.visible = False


# =============================================================================
# 4. User Action Dialog Pattern (Dialog with Actionable Buttons)
# =============================================================================


class ErrorDialog(Widget):
    """
    Error dialog with actionable buttons.

    Per ADR 0014:
    - Shows error with context and details
    - Provides actionable buttons for user response
    - No automatic retry - user decides

    Example:
        self.show_error_dialog(
            title="Worker Spawn Failed",
            message=error_message,
            actions=[
                ("View Logs", self.view_logs),
                ("Edit Config", self.edit_config),
                ("Retry", self.retry_spawn),
                ("Dismiss", None),
            ]
        )
    """

    DEFAULT_CSS = """
    ErrorDialog {
        layer: overlay;
        dock: top;
        height: 25;
        padding: 1 2;
    }

    .dialog-container {
        background: $surface;
        border: thick $error;
        border-subtitle: "Error";
        padding: 1;
        height: 100%;
    }

    .dialog-title {
        text-style: bold;
        color: $error;
        margin: 0 0 1 0;
    }

    .dialog-message {
        margin: 0 0 1 0;
        padding: 1;
        background: $panel;
        border: round $primary;
    }

    .dialog-details {
        margin: 1 0;
        padding: 1;
        background: $panel;
        border: round $primary;
        height: 10;
        overflow-y: auto;
    }

    .dialog-details-title {
        text-style: bold;
        margin: 0 0 1 0;
    }

    .dialog-buttons {
        dock: bottom;
        height: 3;
        align: center_middle;
    }

    Button {
        margin: 0 1;
    }

    .dialog-error-icon {
        text-align: center;
        margin: 0 0 1 0;
        color: $error;
    }
    """

    showing: reactive[bool] = False
    title: reactive[str] = reactive("Error")
    message: reactive[str] = reactive("")
    details: reactive[dict[str, Any]] = reactive(dict)
    actions: reactive[list[ErrorAction]] = reactive(list)
    _callback: Callable[[str | None], None] | None = None

    def __init__(self, **kwargs: Any) -> None:
        super().__init__(**kwargs)
        self._setup_ui()

    def _setup_ui(self) -> None:
        """Setup the UI components."""
        self.container = Container(classes="dialog-container")

        # Icon and title
        self.icon = Static("⚠️", classes="dialog-error-icon")
        self.title_display = Static("", classes="dialog-title")

        # Message
        self.message_display = Static("", classes="dialog-message")

        # Details
        self.details_container = Vertical(classes="dialog-details")
        self.details_title = Static("Details:", classes="dialog-details-title")
        self.details_content = Static("", classes="dialog-details-content")
        self.details_container.mount(self.details_title, self.details_content)

        # Buttons
        self.button_container = Horizontal(classes="dialog-buttons")

        # Mount everything
        self.container.mount(
            self.icon,
            self.title_display,
            self.message_display,
            self.details_container,
            self.button_container
        )
        self.mount(self.container)

    def show(
        self,
        title: str,
        message: str,
        details: dict[str, Any] | None = None,
        actions: list[ErrorAction] | None = None,
        callback: Callable[[str | None], None] | None = None
    ) -> None:
        """
        Show error dialog.

        Args:
            title: Error title
            message: Error message
            details: Additional details (component, context, etc.)
            actions: List of actionable buttons
            callback: Optional callback with action label
        """
        self.title = title
        self.message = message
        self.details = details or {}
        self.actions = actions or []
        self._callback = callback
        self.showing = True
        self.visible = True

        self._update_display()
        self._rebuild_buttons()

    def hide(self) -> None:
        """Hide the dialog."""
        self.showing = False
        self.visible = False
        self.title = "Error"
        self.message = ""
        self.details = {}
        self.actions = []
        self._callback = None

    def _update_display(self) -> None:
        """Update the display."""
        # Update title
        self.title_display.update(self.title)

        # Update message
        self.message_display.update(self.message)

        # Update details
        if self.details:
            self.details_container.visible = True
            lines = []
            for key, value in self.details.items():
                if isinstance(value, (list, dict)):
                    value_str = str(value)[:200]
                else:
                    value_str = str(value)
                lines.append(f"{key}: {value_str}")
            self.details_content.update("\n".join(lines))
        else:
            self.details_container.visible = False

    def _rebuild_buttons(self) -> None:
        """Rebuild buttons based on actions."""
        # Remove existing buttons
        self.button_container.remove_children()

        # Add new buttons
        for action in self.actions:
            button = Button(action.label, variant=action.variant)
            button.action_label = action.label  # Store for callback
            button.action_callback = action.callback  # Store callback
            self.button_container.mount(button)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        """Handle button press."""
        button = event.button

        # Get action label and callback
        action_label = getattr(button, "action_label", None)
        action_callback = getattr(button, "action_callback", None)

        # Call action callback if provided
        if action_callback:
            action_callback()

        # Call dialog callback if provided
        if self._callback:
            self._callback(action_label)

        # Hide dialog
        self.hide()

    def on_key(self, event) -> None:
        """Handle keyboard shortcuts."""
        if not self.showing:
            return

        if event.key == "escape":
            self.hide()
            if self._callback:
                self._callback(None)


# =============================================================================
# Error Display Manager (Convenience API)
# =============================================================================


class ErrorDisplayManager:
    """
    Convenience API for displaying errors per ADR 0014.

    Usage:
        manager = ErrorDisplayManager(app)
        manager.transient("Cost update delayed", severity="warning")
        manager.component("chat", "Backend unavailable", fallback="Hotkey mode")
        manager.fatal("Cannot Start", ["Permission denied"], ["Fix perms..."])
        manager.dialog("Worker Failed", "Launcher error", actions=[...])
    """

    def __init__(self, app):
        """Initialize error display manager with app instance."""
        self.app = app
        self._notification: NotificationOverlay | None = None
        self._fatal_screen: FatalErrorScreen | None = None
        self._error_dialog: ErrorDialog | None = None
        self._component_errors: dict[str, ComponentErrorWidget] = {}

    def _get_notification(self) -> NotificationOverlay:
        """Get or create notification overlay."""
        if self._notification is None:
            self._notification = NotificationOverlay()
            self.app.mount(self._notification)
        return self._notification

    def _get_fatal_screen(self) -> FatalErrorScreen:
        """Get or create fatal error screen."""
        if self._fatal_screen is None:
            self._fatal_screen = FatalErrorScreen()
            self.app.mount(self._fatal_screen)
        return self._fatal_screen

    def _get_error_dialog(self) -> ErrorDialog:
        """Get or create error dialog."""
        if self._error_dialog is None:
            self._error_dialog = ErrorDialog()
            self.app.mount(self._error_dialog)
        return self._error_dialog

    def transient(
        self,
        message: str,
        severity: str = "info",
        timeout: float | None = None
    ) -> None:
        """
        Show transient notification (non-blocking).

        Args:
            message: Notification message
            severity: info, warning, error, success
            timeout: Auto-dismiss timeout in seconds
        """
        notification = self._get_notification()
        notification.show(message, severity=severity, timeout=timeout)

    def component(
        self,
        component_name: str,
        error: str,
        fallback: str = "",
        guidance: list[str] | None = None,
        panel_widget: Widget | None = None
    ) -> None:
        """
        Show component error in panel.

        Args:
            component_name: Name of component (e.g., "chat", "workers")
            error: Error message
            fallback: Fallback mode description
            guidance: List of actionable guidance
            panel_widget: Panel widget to mount error in (optional)
        """
        # Get or create component error widget
        if component_name not in self._component_errors:
            error_widget = ComponentErrorWidget(component=component_name)
            self._component_errors[component_name] = error_widget

            # Mount to panel if provided
            if panel_widget:
                panel_widget.mount(error_widget)
        else:
            error_widget = self._component_errors[component_name]

        # Show error
        error_widget.show_error(error, fallback, guidance)

    def clear_component(self, component_name: str) -> None:
        """Clear component error (recovered)."""
        if component_name in self._component_errors:
            self._component_errors[component_name].clear_error()

    def fatal(
        self,
        title: str,
        errors: list[str],
        guidance: list[str],
        exit_on_dismiss: bool = True
    ) -> None:
        """
        Show fatal error screen (blocking).

        Args:
            title: Error title
            errors: List of error messages
            guidance: List of fix suggestions
            exit_on_dismiss: Whether to exit app on dismiss
        """
        fatal_screen = self._get_fatal_screen()
        fatal_screen.show(title, errors, guidance, exit_on_dismiss)

    def dialog(
        self,
        title: str,
        message: str,
        details: dict[str, Any] | None = None,
        actions: list[ErrorAction] | None = None
    ) -> None:
        """
        Show error dialog with actions.

        Args:
            title: Error title
            message: Error message
            details: Additional context
            actions: List of actionable buttons
        """
        error_dialog = self._get_error_dialog()
        error_dialog.show(title, message, details, actions)
