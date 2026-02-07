#!/usr/bin/env python3
"""
FORGE Worker Configuration Validator

Validates worker configuration files for correctness.

Usage:
    ./worker-config-validator.py <config-file>
    ./worker-config-validator.py ~/.forge/workers/claude-code-sonnet.yaml
"""

import sys
import yaml
from pathlib import Path
from typing import Any, Dict, List

class ConfigValidator:
    def __init__(self, config_path: str):
        self.config_path = Path(config_path).expanduser()
        self.config = None
        self.errors = []
        self.warnings = []

    def load_config(self) -> bool:
        """Load and parse config file"""
        print(f"Loading config: {self.config_path}...", end=" ")

        if not self.config_path.exists():
            print(f"❌ FAIL - File not found")
            return False

        try:
            with open(self.config_path) as f:
                self.config = yaml.safe_load(f)
            print("✅ PASS")
            return True
        except yaml.YAMLError as e:
            print(f"❌ FAIL - Invalid YAML: {e}")
            return False
        except Exception as e:
            print(f"❌ FAIL - {e}")
            return False

    def validate_required_fields(self) -> bool:
        """Validate required fields exist"""
        print("Checking required fields...", end=" ")

        required = ["name", "launcher", "model", "tier"]
        missing = []

        for field in required:
            if field not in self.config:
                missing.append(field)

        if missing:
            print(f"❌ FAIL - Missing: {', '.join(missing)}")
            return False

        print("✅ PASS")
        return True

    def validate_tier(self) -> bool:
        """Validate tier field"""
        print("Checking tier...", end=" ")

        tier = self.config.get("tier")
        valid_tiers = ["premium", "standard", "budget", "free"]

        if tier not in valid_tiers:
            print(f"❌ FAIL - Invalid tier '{tier}' (must be: {', '.join(valid_tiers)})")
            return False

        print("✅ PASS")
        return True

    def validate_cost(self) -> bool:
        """Validate cost information"""
        print("Checking cost information...", end=" ")

        if "cost_per_million_tokens" in self.config:
            cost = self.config["cost_per_million_tokens"]

            if not isinstance(cost, dict):
                print(f"❌ FAIL - cost_per_million_tokens must be object")
                return False

            if "input" not in cost or "output" not in cost:
                print(f"❌ FAIL - Missing input/output costs")
                return False

            if not isinstance(cost["input"], (int, float)):
                print(f"❌ FAIL - input cost must be number")
                return False

            if not isinstance(cost["output"], (int, float)):
                print(f"❌ FAIL - output cost must be number")
                return False

        print("✅ PASS")
        return True

    def validate_subscription(self) -> bool:
        """Validate subscription information"""
        print("Checking subscription...", end=" ")

        if "subscription" in self.config:
            sub = self.config["subscription"]

            if not isinstance(sub, dict):
                print(f"❌ FAIL - subscription must be object")
                return False

            if "enabled" not in sub:
                print(f"❌ FAIL - subscription missing 'enabled' field")
                return False

            if sub["enabled"]:
                if "monthly_cost" not in sub:
                    print(f"❌ FAIL - subscription missing 'monthly_cost'")
                    return False

        print("✅ PASS")
        return True

    def validate_environment(self) -> bool:
        """Validate environment variables"""
        print("Checking environment...", end=" ")

        if "environment" in self.config:
            env = self.config["environment"]

            if not isinstance(env, dict):
                print(f"❌ FAIL - environment must be object")
                return False

            # Check for sensitive data
            for key, value in env.items():
                if isinstance(value, str):
                    # Should use ${VAR} syntax, not hardcoded secrets
                    if any(secret in value for secret in ["sk-", "key-", "token-", "secret-"]):
                        if not value.startswith("${"):
                            self.warnings.append(
                                f"⚠️  WARNING - Hardcoded secret in {key}? Use ${{{key}}} instead"
                            )

        print("✅ PASS")
        return True

    def validate_spawn_args(self) -> bool:
        """Validate spawn arguments"""
        print("Checking spawn_args...", end=" ")

        if "spawn_args" in self.config:
            args = self.config["spawn_args"]

            if not isinstance(args, list):
                print(f"❌ FAIL - spawn_args must be array")
                return False

            # Check for variable placeholders
            for arg in args:
                if not isinstance(arg, str):
                    print(f"❌ FAIL - spawn_args must be strings")
                    return False

        print("✅ PASS")
        return True

    def validate_paths(self) -> bool:
        """Validate file paths"""
        print("Checking file paths...", end=" ")

        for path_field in ["log_path", "status_path"]:
            if path_field in self.config:
                path = self.config[path_field]

                if not isinstance(path, str):
                    print(f"❌ FAIL - {path_field} must be string")
                    return False

                # Should contain ${worker_id} placeholder
                if "${worker_id}" not in path:
                    self.warnings.append(
                        f"⚠️  WARNING - {path_field} should contain ${{worker_id}} placeholder"
                    )

        print("✅ PASS")
        return True

    def run_all_validations(self) -> bool:
        """Run all validations"""
        print(f"\n{'='*60}")
        print(f"Validating worker config: {self.config_path}")
        print(f"{'='*60}\n")

        if not self.load_config():
            return False

        validations = [
            self.validate_required_fields,
            self.validate_tier,
            self.validate_cost,
            self.validate_subscription,
            self.validate_environment,
            self.validate_spawn_args,
            self.validate_paths,
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

        if self.warnings:
            print(f"\nWarnings:")
            for warning in self.warnings:
                print(f"  {warning}")

        print(f"{'='*60}\n")

        return failed == 0


def main():
    if len(sys.argv) < 2:
        print("Usage: worker-config-validator.py <config-file>")
        print()
        print("Examples:")
        print("  ./worker-config-validator.py ~/.forge/workers/claude-code-sonnet.yaml")
        print("  ./worker-config-validator.py /path/to/my-worker.yaml")
        sys.exit(1)

    config_path = sys.argv[1]

    validator = ConfigValidator(config_path)
    success = validator.run_all_validations()

    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
