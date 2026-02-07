#!/usr/bin/env python3
"""
FORGE Status File Validator

Validates worker status files for correct format.

Usage:
    ./status-file-validator.py <status-file>
    ./status-file-validator.py ~/.forge/status/sonnet-alpha.json
"""

import json
import sys
from pathlib import Path
from datetime import datetime

class StatusValidator:
    def __init__(self, status_path: str):
        self.status_path = Path(status_path).expanduser()
        self.status = None

    def load_status(self) -> bool:
        """Load status file"""
        print(f"Loading status file: {self.status_path}...", end=" ")

        if not self.status_path.exists():
            print(f"❌ FAIL - File not found")
            return False

        try:
            with open(self.status_path) as f:
                self.status = json.load(f)
            print("✅ PASS")
            return True
        except json.JSONDecodeError as e:
            print(f"❌ FAIL - Invalid JSON: {e}")
            return False
        except Exception as e:
            print(f"❌ FAIL - {e}")
            return False

    def validate_required_fields(self) -> bool:
        """Validate required fields"""
        print("Checking required fields...", end=" ")

        required = ["worker_id", "status", "model", "workspace"]
        missing = []

        for field in required:
            if field not in self.status:
                missing.append(field)

        if missing:
            print(f"❌ FAIL - Missing: {', '.join(missing)}")
            return False

        print("✅ PASS")
        return True

    def validate_status_field(self) -> bool:
        """Validate status field"""
        print("Checking status field...", end=" ")

        status = self.status.get("status")
        valid_statuses = ["active", "idle", "failed", "stopped", "starting", "spawned"]

        if status not in valid_statuses:
            print(f"❌ FAIL - Invalid status '{status}' (must be: {', '.join(valid_statuses)})")
            return False

        print("✅ PASS")
        return True

    def validate_timestamps(self) -> bool:
        """Validate timestamp fields"""
        print("Checking timestamps...", end=" ")

        for field in ["started_at", "last_activity"]:
            if field in self.status:
                try:
                    datetime.fromisoformat(self.status[field].replace('Z', '+00:00'))
                except:
                    print(f"❌ FAIL - Invalid {field} format (expected ISO 8601)")
                    return False

        print("✅ PASS")
        return True

    def validate_types(self) -> bool:
        """Validate field types"""
        print("Checking field types...", end=" ")

        # String fields
        for field in ["worker_id", "status", "model", "workspace"]:
            if field in self.status:
                if not isinstance(self.status[field], str):
                    print(f"❌ FAIL - {field} must be string")
                    return False

        # Integer fields
        for field in ["pid", "tasks_completed"]:
            if field in self.status:
                if not isinstance(self.status[field], int):
                    print(f"❌ FAIL - {field} must be integer")
                    return False

        print("✅ PASS")
        return True

    def validate_consistency(self) -> bool:
        """Validate logical consistency"""
        print("Checking consistency...", end=" ")

        # If stopped, should have stopped_at
        if self.status.get("status") == "stopped":
            if "stopped_at" not in self.status:
                print(f"⚠️  WARNING - Stopped worker should have 'stopped_at' timestamp")

        # If active, should have recent last_activity
        if self.status.get("status") == "active":
            if "last_activity" in self.status:
                try:
                    last_activity = datetime.fromisoformat(
                        self.status["last_activity"].replace('Z', '+00:00')
                    )
                    now = datetime.now(last_activity.tzinfo)
                    age_minutes = (now - last_activity).total_seconds() / 60

                    if age_minutes > 60:
                        print(f"⚠️  WARNING - Active worker but last_activity is {age_minutes:.0f} minutes old")
                except:
                    pass

        print("✅ PASS")
        return True

    def run_all_validations(self) -> bool:
        """Run all validations"""
        print(f"\n{'='*60}")
        print(f"Validating status file: {self.status_path}")
        print(f"{'='*60}\n")

        if not self.load_status():
            return False

        validations = [
            self.validate_required_fields,
            self.validate_status_field,
            self.validate_timestamps,
            self.validate_types,
            self.validate_consistency,
        ]

        passed = 0
        failed = 0

        for validation in validations:
            try:
                if validation():
                    passed += 1
                else:
                    failed += 1
            except Exception as e:
                print(f"❌ EXCEPTION: {e}")
                failed += 1

        print(f"\n{'='*60}")
        print(f"Results: {passed} passed, {failed} failed")
        print(f"{'='*60}\n")

        if failed == 0 and "--show" in sys.argv:
            print("Status contents:")
            print(json.dumps(self.status, indent=2))
            print()

        return failed == 0


def main():
    if len(sys.argv) < 2:
        print("Usage: status-file-validator.py <status-file>")
        print()
        print("Examples:")
        print("  ./status-file-validator.py ~/.forge/status/sonnet-alpha.json")
        print("  ./status-file-validator.py /path/to/status.json")
        print()
        print("Options:")
        print("  --show  Show status file contents after validation")
        sys.exit(1)

    status_path = sys.argv[1]

    validator = StatusValidator(status_path)
    success = validator.run_all_validations()

    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
