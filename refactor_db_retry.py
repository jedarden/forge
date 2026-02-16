#!/usr/bin/env python3
"""
Script to refactor database methods to use with_retry wrapper.
"""
import re
import sys

def refactor_method(content, start_line, method_name):
    """Refactor a single method to wrap the body in with_retry."""
    lines = content.split('\n')

    # Find the method signature line
    sig_idx = None
    for i, line in enumerate(lines):
        if f'pub fn {method_name}(' in line:
            sig_idx = i
            break

    if sig_idx is None:
        return content

    # Find where the actual implementation starts (after the opening brace)
    impl_start = None
    for i in range(sig_idx, len(lines)):
        if lines[i].strip().endswith('{'):
            impl_start = i + 1
            break
        if '{' in lines[i]:
            impl_start = i + 1
            break

    if impl_start is None:
        return content

    # Check if already wrapped with with_retry
    if 'with_retry' in lines[impl_start:impl_start+5]:
        print(f"Method {method_name} already wrapped")
        return content

    # Find the corresponding closing brace
    brace_count = 1
    impl_end = impl_start
    for i in range(impl_start, len(lines)):
        brace_count += lines[i].count('{')
        brace_count -= lines[i].count('}')
        if brace_count == 0:
            impl_end = i
            break

    # Get the indentation of the first line
    indent = len(lines[impl_start]) - len(lines[impl_start].lstrip())
    indent_str = ' ' * indent

    # Wrap the method body
    new_lines = lines[:impl_start]
    new_lines.append(f'{indent_str}self.with_retry("{method_name}", || {{')

    # Indent the existing body
    for i in range(impl_start, impl_end):
        new_lines.append('    ' + lines[i])

    new_lines.append(f'{indent_str}}})')
    new_lines.extend(lines[impl_end:])

    return '\n'.join(new_lines)

# List of methods to refactor
methods_to_refactor = [
    'get_last_timestamp',
    'exists',
    'upsert_subscription',
    'get_subscription',
    'get_active_subscriptions',
    'get_all_subscriptions',
    'update_subscription_usage',
    'increment_subscription_usage',
    'record_subscription_usage',
    'get_subscription_period_usage',
    'deactivate_subscription',
    'aggregate_hourly_stats',
    'aggregate_daily_stats',
    'aggregate_worker_efficiency',
    'aggregate_model_performance',
    'get_hourly_stat',
    'get_daily_stat',
    'get_worker_efficiency',
    'get_model_performance',
]

if __name__ == '__main__':
    db_file = '/home/coder/forge/crates/forge-cost/src/db.rs'

    with open(db_file, 'r') as f:
        content = f.read()

    print(f"Refactoring {len(methods_to_refactor)} methods...")

    for method in methods_to_refactor:
        print(f"Refactoring {method}...")
        content = refactor_method(content, 0, method)

    with open(db_file, 'w') as f:
        f.write(content)

    print("Refactoring complete!")
