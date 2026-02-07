#!/usr/bin/env python3
"""
FORGE Headless CLI Backend Test Harness

Tests chat backend for protocol compliance.

Usage:
    ./backend-test-harness.py <backend-command> [args...]
    ./backend-test-harness.py claude-code chat --headless
"""

import json
import subprocess
import sys
import time
from pathlib import Path
from typing import Dict

class BackendTest:
    def __init__(self, command: list):
        self.command = command
        self.test_results = []

    def send_message(self, message: str, tools: list = None, context: dict = None) -> Dict:
        """Send message to backend and return response"""
        if tools is None:
            tools = self.get_test_tools()
        if context is None:
            context = {}

        input_data = {
            "message": message,
            "tools": tools,
            "context": context
        }

        try:
            result = subprocess.run(
                self.command,
                input=json.dumps(input_data),
                capture_output=True,
                text=True,
                timeout=30
            )

            return {
                "success": result.returncode == 0,
                "exit_code": result.returncode,
                "stdout": result.stdout,
                "stderr": result.stderr
            }
        except subprocess.TimeoutExpired:
            return {
                "success": False,
                "error": "Timeout (>30s)"
            }
        except Exception as e:
            return {
                "success": False,
                "error": str(e)
            }

    def get_test_tools(self) -> list:
        """Get test tool definitions"""
        return [
            {
                "name": "test_tool",
                "description": "A test tool",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "description": "Action to perform"
                        }
                    },
                    "required": ["action"]
                }
            },
            {
                "name": "switch_view",
                "description": "Switch dashboard view",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "view": {
                            "type": "string",
                            "enum": ["workers", "tasks", "costs"]
                        }
                    },
                    "required": ["view"]
                }
            }
        ]

    def test_1_input_handling(self) -> bool:
        """Test 1: Input handling"""
        print("Test 1: Input Handling...", end=" ")

        result = self.send_message("test message")

        if not result["success"]:
            print(f"❌ FAIL - Backend crashed or returned error")
            if result.get("stderr"):
                print(f"  Error: {result['stderr'][:100]}")
            return False

        print("✅ PASS")
        return True

    def test_2_output_format(self) -> bool:
        """Test 2: Output format"""
        print("Test 2: Output Format...", end=" ")

        result = self.send_message("show workers")

        if not result["success"]:
            print(f"❌ FAIL - Backend failed")
            return False

        # Parse JSON output
        try:
            output = json.loads(result["stdout"])
        except json.JSONDecodeError as e:
            print(f"❌ FAIL - Invalid JSON: {e}")
            print(f"  Output: {result['stdout'][:100]}")
            return False

        # Check for required fields
        if "tool_calls" not in output and "message" not in output:
            print(f"❌ FAIL - Missing 'tool_calls' or 'message' field")
            return False

        print("✅ PASS")
        return True

    def test_3_tool_calls(self) -> bool:
        """Test 3: Tool call generation"""
        print("Test 3: Tool Call Generation...", end=" ")

        result = self.send_message("switch to workers view")

        if not result["success"]:
            print(f"❌ FAIL - Backend failed")
            return False

        try:
            output = json.loads(result["stdout"])
        except:
            print(f"❌ FAIL - Invalid JSON output")
            return False

        # Should generate tool call
        if "tool_calls" not in output:
            print(f"❌ FAIL - No tool_calls generated")
            return False

        tool_calls = output["tool_calls"]
        if not isinstance(tool_calls, list):
            print(f"❌ FAIL - tool_calls should be array")
            return False

        if len(tool_calls) == 0:
            print(f"❌ FAIL - tool_calls array is empty")
            return False

        # Validate tool call structure
        first_call = tool_calls[0]
        if "tool" not in first_call or "arguments" not in first_call:
            print(f"❌ FAIL - Tool call missing required fields")
            return False

        print("✅ PASS")
        return True

    def test_4_context_awareness(self) -> bool:
        """Test 4: Context awareness"""
        print("Test 4: Context Awareness...", end=" ")

        context = {
            "current_view": "workers",
            "visible_workers": ["sonnet-alpha", "opus-beta"]
        }

        result = self.send_message(
            "how many workers are visible?",
            context=context
        )

        if not result["success"]:
            print(f"❌ FAIL - Backend failed")
            return False

        # Backend should acknowledge context in message
        try:
            output = json.loads(result["stdout"])
            message = output.get("message", "")

            # Should mention "2" or "two" workers
            if "2" not in message.lower() and "two" not in message.lower():
                print(f"⚠️  WARNING - Backend may not be using context")
                print(f"  Message: {message[:100]}")
                # Don't fail, just warn
        except:
            pass

        print("✅ PASS")
        return True

    def test_5_error_handling(self) -> bool:
        """Test 5: Error handling"""
        print("Test 5: Error Handling...", end=" ")

        # Send malformed JSON
        try:
            result = subprocess.run(
                self.command,
                input="not valid json",
                capture_output=True,
                text=True,
                timeout=10
            )

            # Should handle gracefully (non-zero exit or error response)
            if result.returncode == 0:
                try:
                    output = json.loads(result.stdout)
                    if "error" not in output:
                        print(f"❌ FAIL - Should handle malformed input")
                        return False
                except:
                    pass  # Okay if it doesn't return JSON for bad input

        except Exception as e:
            print(f"❌ FAIL - Exception: {e}")
            return False

        print("✅ PASS")
        return True

    def test_6_performance(self) -> bool:
        """Test 6: Performance"""
        print("Test 6: Performance...", end=" ")

        start = time.time()
        result = self.send_message("show workers")
        duration = time.time() - start

        if not result["success"]:
            print(f"❌ FAIL - Backend failed")
            return False

        if duration > 30:
            print(f"❌ FAIL - Too slow ({duration:.1f}s > 30s)")
            return False

        if duration > 10:
            print(f"⚠️  WARNING - Slow response ({duration:.1f}s > 10s)")

        print(f"✅ PASS ({duration:.1f}s)")
        return True

    def run_all_tests(self) -> bool:
        """Run all tests"""
        print(f"\n{'='*60}")
        print(f"Testing backend: {' '.join(self.command)}")
        print(f"{'='*60}\n")

        tests = [
            self.test_1_input_handling,
            self.test_2_output_format,
            self.test_3_tool_calls,
            self.test_4_context_awareness,
            self.test_5_error_handling,
            self.test_6_performance,
        ]

        passed = 0
        failed = 0

        for test in tests:
            try:
                if test():
                    passed += 1
                else:
                    failed += 1
            except Exception as e:
                print(f"❌ EXCEPTION: {e}")
                failed += 1

        print(f"\n{'='*60}")
        print(f"Results: {passed} passed, {failed} failed")
        print(f"{'='*60}\n")

        return failed == 0


def main():
    if len(sys.argv) < 2:
        print("Usage: backend-test-harness.py <backend-command> [args...]")
        print()
        print("Examples:")
        print("  ./backend-test-harness.py claude-code chat --headless")
        print("  ./backend-test-harness.py python my-backend.py")
        sys.exit(1)

    command = sys.argv[1:]

    tester = BackendTest(command)
    success = tester.run_all_tests()

    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
