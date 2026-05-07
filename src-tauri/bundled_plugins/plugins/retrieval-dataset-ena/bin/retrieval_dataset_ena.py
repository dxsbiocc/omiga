#!/usr/bin/env python3
"""Wrapper for a single Omiga retrieval data-source plugin."""

from pathlib import Path
import runpy


def find_runner(filename: str) -> Path:
    here = Path(__file__).resolve()
    for parent in here.parents:
        candidate = parent / "source_runners" / filename
        if candidate.is_file():
            return candidate
    raise FileNotFoundError(f"source runner {filename} not found near {here}")


RUNNER = find_runner("public_dataset_sources.py")
runpy.run_path(str(RUNNER), run_name="__main__")
