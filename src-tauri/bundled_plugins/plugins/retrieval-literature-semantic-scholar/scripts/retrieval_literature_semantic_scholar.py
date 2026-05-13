#!/usr/bin/env python3
"""Wrapper for a single Omiga retrieval resource plugin."""

from pathlib import Path
import runpy


def find_runner(filename: str) -> Path:
    here = Path(__file__).resolve()
    for parent in here.parents:
        for directory in ("resource_runners", "source_runners"):
            candidate = parent / directory / filename
            if candidate.is_file():
                return candidate
    raise FileNotFoundError(f"resource runner {filename} not found near {here}")


RUNNER = find_runner("public_literature_sources.py")
runpy.run_path(str(RUNNER), run_name="__main__")
