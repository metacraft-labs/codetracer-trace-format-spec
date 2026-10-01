"""Test program for streaming recording verification.

Alternates between sleep and burst activity to test that the CodeTracer GUI
renders all phases of execution correctly in the event log, call trace, and
variable state panes.
"""
import time
import sys


def compute_fibonacci(n):
    """A function that does visible work."""
    a, b = 0, 1
    for i in range(n):
        a, b = b, a + b
    return a


def burst_activity(label, count=5):
    """Execute a burst of function calls and print output."""
    print(f"[{label}] Starting burst", flush=True)
    results = []
    for i in range(count):
        result = compute_fibonacci(10 + i)
        results.append(result)
    print(f"[{label}] Results: {results}", flush=True)
    return results


# Phase 1: Initial burst
burst_activity("phase1", 3)

# Phase 2: Sleep then burst
time.sleep(0.5)
burst_activity("phase2", 3)

# Phase 3: Sleep then final burst with print
time.sleep(0.5)
burst_activity("phase3", 3)
print("All phases complete", flush=True)
