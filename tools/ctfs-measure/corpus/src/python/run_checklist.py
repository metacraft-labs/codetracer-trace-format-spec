"""Driver for codetracer/test-programs/py_checklist (copied under ./codetracer-test-programs/test-programs).

Runs the checklist modules' run_all() in the checklist's own order; module names
given on the command line restrict the set (default: all).
"""
import os
import sys
from importlib import import_module

here = os.path.join(os.path.dirname(os.path.abspath(__file__)), "codetracer-test-programs")
sys.path.insert(0, here)

ORDER = ["basics", "functions_exceptions", "contexts_iterators", "async_concurrency", "data_model",
         "collections_dataclasses", "system_utils", "introspection", "imports_demo",
         "advanced_runtime", "miscellaneous"]

wanted = sys.argv[1:] or ORDER
for name in ORDER:
    if name in wanted:
        print(f"== Running {name} ==")
        import_module(f"test-programs.py_checklist.{name}").run_all()
