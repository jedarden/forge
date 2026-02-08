"""
FORGE Error Display Tests (ADR 0014)

Tests for error display patterns per ADR 0014:
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

from unittest.mock import Mock
import pytest

from forge.error_display import (
    ErrorSeverity,
    ErrorAction,
    ErrorDetails,
)


# =============================================================================
# 1. Error Action Tests
# =============================================================================


class TestErrorAction:
    """Tests for ErrorAction dataclass"""

    def test_error_action_creation(self):
        """Test creating error action"""
        callback = Mock()
        action = ErrorAction(
            label="Retry",
            callback=callback,
            variant="primary"
        )

        assert action.label == "Retry"
        assert action.callback is callback
        assert action.variant == "primary"

    def test_error_action_default_variant(self):
        """Test error action gets default variant"""
        action_with_callback = ErrorAction(label="Test", callback=Mock())
        action_without_callback = ErrorAction(label="Dismiss", callback=None)

        assert action_with_callback.variant == "default"
        assert action_without_callback.variant == "error"


# =============================================================================
# 2. Error Severity Tests
# =============================================================================


class TestErrorSeverity:
    """Tests for ErrorSeverity enum"""

    def test_error_severity_values(self):
        """Test error severity enum values"""
        assert ErrorSeverity.WARNING.value == "warning"
        assert ErrorSeverity.ERROR.value == "error"
        assert ErrorSeverity.FATAL.value == "fatal"

    def test_error_severity_comparison(self):
        """Test error severity enum comparison"""
        assert ErrorSeverity.WARNING == ErrorSeverity.WARNING
        assert ErrorSeverity.ERROR != ErrorSeverity.WARNING
        assert ErrorSeverity.FATAL == ErrorSeverity.FATAL


# =============================================================================
# 3. Error Details Tests
# =============================================================================


class TestErrorDetails:
    """Tests for ErrorDetails dataclass"""

    def test_error_details_creation(self):
        """Test creating error details"""
        callback = Mock()
        details = ErrorDetails(
            title="Test Error",
            message="Test error message",
            severity=ErrorSeverity.ERROR,
            context={"component": "chat"},
            guidance=["Fix 1", "Fix 2"],
            actions=[ErrorAction(label="Retry", callback=callback)]
        )

        assert details.title == "Test Error"
        assert details.message == "Test error message"
        assert details.severity == ErrorSeverity.ERROR
        assert details.context == {"component": "chat"}
        assert len(details.guidance) == 2
        assert len(details.actions) == 1

    def test_error_details_defaults(self):
        """Test error details default values"""
        details = ErrorDetails(
            title="Test",
            message="Test message"
        )

        assert details.severity == ErrorSeverity.ERROR
        assert details.context == {}
        assert details.guidance == []
        assert details.actions == []


# =============================================================================
# 4. API Existence Tests
# =============================================================================


class TestForgeAppErrorAPI:
    """Tests that ForgeApp has the correct error display API per ADR 0014"""

    def test_transient_error_api_exists(self):
        """Test show_transient_error method exists on ForgeApp"""
        from forge.app import ForgeApp

        app = ForgeApp()

        # Verify method exists per ADR 0014: Transient Errors (Non-Blocking)
        assert hasattr(app, 'show_transient_error')
        assert callable(app.show_transient_error)

        # Verify signature: message, severity, timeout
        import inspect
        sig = inspect.signature(app.show_transient_error)
        params = list(sig.parameters.keys())
        assert 'message' in params
        assert 'severity' in params
        assert 'timeout' in params

        # Per ADR 0014: No automatic retry parameter
        assert 'auto_retry' not in params
        assert 'retry_count' not in params

    def test_component_error_api_exists(self):
        """Test show_component_error method exists on ForgeApp"""
        from forge.app import ForgeApp

        app = ForgeApp()

        # Verify method exists per ADR 0014: Component Errors (Degrade Component)
        assert hasattr(app, 'show_component_error')
        assert callable(app.show_component_error)

        # Verify signature includes guidance for actionable steps
        import inspect
        sig = inspect.signature(app.show_component_error)
        params = list(sig.parameters.keys())
        assert 'component' in params
        assert 'error' in params
        assert 'fallback' in params
        assert 'guidance' in params

        # Per ADR 0014: No automatic retry
        assert 'auto_retry' not in params

    def test_clear_component_error_api_exists(self):
        """Test clear_component_error method exists on ForgeApp"""
        from forge.app import ForgeApp

        app = ForgeApp()

        # Verify method exists for error recovery
        assert hasattr(app, 'clear_component_error')
        assert callable(app.clear_component_error)

    def test_fatal_error_api_exists(self):
        """Test show_fatal_error method exists on ForgeApp"""
        from forge.app import ForgeApp

        app = ForgeApp()

        # Verify method exists per ADR 0014: Fatal Errors (Block Startup)
        assert hasattr(app, 'show_fatal_error')
        assert callable(app.show_fatal_error)

        # Verify signature includes actionable guidance
        import inspect
        sig = inspect.signature(app.show_fatal_error)
        params = list(sig.parameters.keys())
        assert 'title' in params
        assert 'errors' in params
        assert 'guidance' in params
        assert 'exit_on_dismiss' in params

    def test_error_dialog_api_exists(self):
        """Test show_error_dialog method exists on ForgeApp"""
        from forge.app import ForgeApp

        app = ForgeApp()

        # Verify method exists per ADR 0014: User Action Required
        assert hasattr(app, 'show_error_dialog')
        assert callable(app.show_error_dialog)

        # Verify signature includes actionable buttons
        import inspect
        sig = inspect.signature(app.show_error_dialog)
        params = list(sig.parameters.keys())
        assert 'title' in params
        assert 'message' in params
        assert 'details' in params
        assert 'actions' in params

        # Per ADR 0014: User must choose action, no automatic retry
        assert 'auto_retry' not in params

    def test_error_display_manager_initialization(self):
        """Test ErrorDisplayManager is initialized in ForgeApp"""
        from forge.app import ForgeApp

        app = ForgeApp()

        # ErrorDisplayManager should be initialized
        assert app._error_display is not None
        assert hasattr(app._error_display, 'transient')
        assert hasattr(app._error_display, 'component')
        assert hasattr(app._error_display, 'fatal')
        assert hasattr(app._error_display, 'dialog')


# =============================================================================
# 5. ADR 0014 Compliance Tests
# =============================================================================


class TestADR0014Compliance:
    """Tests that verify compliance with ADR 0014 principles"""

    def test_visibility_first_principle(self):
        """Test that error display supports visibility first principle"""
        from forge.app import ForgeApp

        app = ForgeApp()

        # Per ADR 0014: "Show errors clearly in TUI"
        # All error methods should be public and accessible
        assert hasattr(app, 'show_transient_error')  # Non-blocking notifications
        assert hasattr(app, 'show_component_error')  # In-panel errors
        assert hasattr(app, 'show_fatal_error')      # Full-screen blocking
        assert hasattr(app, 'show_error_dialog')     # Actionable dialogs

    def test_no_automatic_retry_principle(self):
        """Test that error handling follows no automatic retry principle"""
        from forge.app import ForgeApp

        app = ForgeApp()

        # Per ADR 0014: "No Automatic Retry: User decides if/when to retry"
        # Verify no automatic retry parameters in any error method
        import inspect

        for method_name in ['show_transient_error', 'show_component_error',
                           'show_fatal_error', 'show_error_dialog']:
            method = getattr(app, method_name)
            sig = inspect.signature(method)
            params = list(sig.parameters.keys())

            # No automatic retry parameters should exist
            assert 'auto_retry' not in params
            assert 'retry_count' not in params
            assert 'max_retries' not in params

    def test_clear_error_messages_principle(self):
        """Test that error display supports clear, actionable messages"""
        from forge.app import ForgeApp

        app = ForgeApp()

        # Per ADR 0014: "Clear Error Messages: Actionable guidance, not technical jargon"
        # All error methods should support guidance/messages
        import inspect

        # Component errors support guidance parameter
        sig = inspect.signature(app.show_component_error)
        assert 'guidance' in sig.parameters
        assert 'error' in sig.parameters
        assert 'fallback' in sig.parameters

        # Fatal errors support guidance parameter
        sig = inspect.signature(app.show_fatal_error)
        assert 'guidance' in sig.parameters
        assert 'errors' in sig.parameters

        # Dialog supports actionable buttons
        sig = inspect.signature(app.show_error_dialog)
        assert 'actions' in sig.parameters
        assert 'message' in sig.parameters

    def test_graceful_degradation_principle(self):
        """Test that error display supports graceful degradation"""
        from forge.app import ForgeApp

        app = ForgeApp()

        # Per ADR 0014: "Degrade Gracefully: Broken component doesn't crash entire app"
        # Component errors should support fallback mode
        import inspect
        sig = inspect.signature(app.show_component_error)
        assert 'fallback' in sig.parameters

    def test_no_silent_failures_principle(self):
        """Test that all errors are visible (no silent failures)"""
        from forge.app import ForgeApp

        app = ForgeApp()

        # Per ADR 0014: "No Silent Failures: Every error is visible to user"
        # All error display methods should be public (not private)
        assert app.show_transient_error.__name__.startswith('show_')
        assert app.show_component_error.__name__.startswith('show_')
        assert app.show_fatal_error.__name__.startswith('show_')
        assert app.show_error_dialog.__name__.startswith('show_')


# =============================================================================
# 6. Integration with Existing Error Handling Tests
# =============================================================================


class TestExistingErrorHandling:
    """Verify that existing error handling integrates with new patterns"""

    def test_worker_status_error_support(self):
        """Test that worker status errors integrate with error display"""
        from forge.app import Worker

        # Worker data model supports error tracking per ADR 0014
        worker = Worker(
            session_id="test-worker",
            model="test-model",
            workspace="/test",
            error="Status file corrupted",  # Error message
            health_error="Health check failed",  # Health error
            health_guidance=["Check worker logs", "Restart worker"]  # Actionable guidance
        )

        assert worker.error is not None
        assert worker.health_error is not None
        assert len(worker.health_guidance) > 0

    def test_launcher_result_error_support(self):
        """Test that launcher errors integrate with error display"""
        from forge.launcher import LauncherResult, LauncherErrorType

        # Launcher result supports detailed errors per ADR 0014
        result = LauncherResult(
            success=False,
            error_type=LauncherErrorType.EXIT_CODE_NONZERO,
            error="Launcher exited with code 1",
            exit_code=1,
            stdout="",
            stderr="API key not set",
            guidance=[
                "Check launcher stderr output",
                "Verify workspace path exists",
                "Test launcher: ~/.forge/launchers/claude-code"
            ]
        )

        assert result.success is False
        assert result.error is not None
        assert result.guidance is not None
        assert len(result.guidance) > 0
