#!/usr/bin/env python3
"""
FORGE Log Format Validator

Validates worker log files for correct format.

Usage:
    ./log-format-validator.py <log-file>
    ./log-format-validator.py ~/.forge/logs/sonnet-alpha.log
"""

import json
import sys
from pathlib import Path
from datetime import datetime

class LogValidator:
    def __init__(self, log_path: str):
        self.log_path = Path(log_path).expanduser()
        self.entries = []
        self.format = None

    def detect_format(self, line: str) -> str:
        """Detect log format (jsonl or keyvalue)"""
        line = line.strip()

        if not line:
            return "empty"

        # Try JSON first
        if line.startswith('{'):
            try:
                json.loads(line)
                return "jsonl"
            except:
                pass

        # Check for key=value format
        if '=' in line and ' ' in line:
            return "keyvalue"

        return "unknown"

    def parse_jsonl(self, line: str) -> dict:
        """Parse JSON line"""
        return json.loads(line.strip())

    def parse_keyvalue(self, line: str) -> dict:
        """Parse key=value line"""
        parts = line.strip().split()
        entry = {}

        # First part is usually timestamp
        if parts:
            entry["timestamp"] = parts[0]

        # Parse key=value pairs
        for part in parts[1:]:
            if '=' in part:
                key, value = part.split('=', 1)
                entry[key] = value.strip('"')

        return entry

    def validate_entry(self, entry: dict, line_num: int) -> list:
        """Validate a single log entry, return list of errors"""
        errors = []

        # Check required fields
        required = ["timestamp", "level", "worker_id"]
        for field in required:
            if field not in entry:
                errors.append(f"Line {line_num}: Missing required field '{field}'")

        # Validate timestamp format
        if "timestamp" in entry:
            try:
                # Try ISO 8601 format
                datetime.fromisoformat(entry["timestamp"].replace('Z', '+00:00'))
            except:
                errors.append(f"Line {line_num}: Invalid timestamp format (expected ISO 8601)")

        # Validate level
        if "level" in entry:
            valid_levels = ["debug", "info", "warning", "error", "critical"]
            if entry["level"].lower() not in valid_levels:
                errors.append(f"Line {line_num}: Invalid level '{entry['level']}' (expected: {', '.join(valid_levels)})")

        # Validate message or event exists
        if "message" not in entry and "event" not in entry:
            errors.append(f"Line {line_num}: Should have 'message' or 'event' field")

        return errors

    def validate_file(self) -> bool:
        """Validate log file"""
        print(f"Validating log file: {self.log_path}\n")

        if not self.log_path.exists():
            print(f"❌ ERROR: File not found: {self.log_path}")
            return False

        if self.log_path.stat().st_size == 0:
            print(f"❌ ERROR: Log file is empty")
            return False

        all_errors = []
        line_num = 0
        format_counts = {"jsonl": 0, "keyvalue": 0, "unknown": 0, "empty": 0}

        with open(self.log_path) as f:
            for line in f:
                line_num += 1

                if not line.strip():
                    format_counts["empty"] += 1
                    continue

                # Detect format
                fmt = self.detect_format(line)
                format_counts[fmt] += 1

                if self.format is None and fmt in ["jsonl", "keyvalue"]:
                    self.format = fmt
                    print(f"Detected format: {fmt.upper()}\n")

                # Parse entry
                try:
                    if fmt == "jsonl":
                        entry = self.parse_jsonl(line)
                    elif fmt == "keyvalue":
                        entry = self.parse_keyvalue(line)
                    else:
                        all_errors.append(f"Line {line_num}: Unknown format")
                        continue

                    # Validate entry
                    errors = self.validate_entry(entry, line_num)
                    all_errors.extend(errors)

                    self.entries.append(entry)

                except Exception as e:
                    all_errors.append(f"Line {line_num}: Parse error - {e}")

        # Report results
        print(f"{'='*60}")
        print(f"Total lines: {line_num}")
        print(f"Valid entries: {len(self.entries)}")
        print(f"Format breakdown: {format_counts}")
        print(f"{'='*60}\n")

        if all_errors:
            print(f"❌ ERRORS FOUND ({len(all_errors)}):\n")
            for error in all_errors[:20]:  # Show first 20 errors
                print(f"  {error}")
            if len(all_errors) > 20:
                print(f"  ... and {len(all_errors) - 20} more errors")
            print()
            return False
        else:
            print("✅ ALL ENTRIES VALID\n")
            return True

    def show_sample_entries(self, count: int = 3):
        """Show sample log entries"""
        if not self.entries:
            return

        print(f"Sample entries ({min(count, len(self.entries))}):\n")

        for i, entry in enumerate(self.entries[:count]):
            print(f"Entry {i+1}:")
            if self.format == "jsonl":
                print(f"  {json.dumps(entry, indent=2)}")
            else:
                for key, value in entry.items():
                    print(f"  {key}: {value}")
            print()


def main():
    if len(sys.argv) < 2:
        print("Usage: log-format-validator.py <log-file>")
        print()
        print("Examples:")
        print("  ./log-format-validator.py ~/.forge/logs/sonnet-alpha.log")
        print("  ./log-format-validator.py /path/to/worker.log")
        sys.exit(1)

    log_path = sys.argv[1]

    validator = LogValidator(log_path)
    success = validator.validate_file()

    if success and "--show-samples" in sys.argv:
        validator.show_sample_entries()

    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
