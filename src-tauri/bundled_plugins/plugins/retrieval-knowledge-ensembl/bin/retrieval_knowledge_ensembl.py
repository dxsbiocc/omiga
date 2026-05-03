#!/usr/bin/env python3
"""Wrapper for a single Omiga retrieval data-source plugin."""

from pathlib import Path
import runpy

RUNNER = Path(__file__).resolve().parents[3] / "source_runners" / "public_knowledge_sources.py"
runpy.run_path(str(RUNNER), run_name="__main__")
