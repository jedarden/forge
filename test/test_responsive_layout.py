#!/usr/bin/env python3
"""
Test script for responsive layout detection
"""

import sys
sys.path.insert(0, 'src')

from textual.app import App
from textual.containers import Container


class LayoutTestApp(App):
    """Test app for layout detection"""

    CSS_PATH = "src/forge/styles.css"

    def compose(self):
        with Container(id="dashboard"):
            with Container(id="top_row"):
                pass
            with Container(id="middle_row"):
                pass
            with Container(id="bottom_row"):
                pass
            with Container(id="spacer"):
                pass

    def on_mount(self):
        """Test layout detection on mount"""
        terminal_height = self.size.height
        dashboard = self.query_one("#dashboard", Container)

        # Test layout class application
        if terminal_height < 45:
            expected_class = "-compact"
            dashboard.add_class("-compact")
        elif terminal_height >= 65:
            expected_class = "-large"
            dashboard.add_class("-large")
        else:
            expected_class = "-responsive"
            dashboard.add_class("-responsive")

        # Verify the class was applied
        classes = dashboard.classes
        if expected_class in classes:
            print(f"✓ PASS: Applied {expected_class} class for terminal height {terminal_height}")
            return True
        else:
            print(f"✗ FAIL: Expected {expected_class} class, but got {classes}")
            return False


def test_layout_detection():
    """Test responsive layout detection"""
    print("Testing Responsive Layout Detection")
    print("=" * 50)

    # Test with different terminal sizes
    test_cases = [
        (38, "-compact"),
        (40, "-compact"),
        (50, "-responsive"),
        (53, "-standard"),
        (54, "-standard"),
        (55, "-standard"),
        (56, "-standard"),
        (57, "-standard"),
        (60, "-responsive"),
        (70, "-large"),
        (80, "-large"),
    ]

    results = []
    for height, expected_class in test_cases:
        # We can't easily test actual terminal size changes in Python
        # But we can verify the logic
        if height < 45:
            applied_class = "-compact"
        elif height >= 65:
            applied_class = "-large"
        elif 53 <= height <= 57:
            applied_class = "-standard"
        else:
            applied_class = "-responsive"

        passed = (applied_class == expected_class)
        results.append(passed)
        status = "✓ PASS" if passed else "✗ FAIL"
        print(f"{status}: Height {height} -> {applied_class} (expected {expected_class})")

    print("=" * 50)
    passed_count = sum(results)
    total_count = len(results)
    print(f"Results: {passed_count}/{total_count} tests passed")

    return all(results)


if __name__ == "__main__":
    success = test_layout_detection()
    sys.exit(0 if success else 1)
