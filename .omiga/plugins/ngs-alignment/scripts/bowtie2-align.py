#!/usr/bin/env python3
from pathlib import Path
import runpy
import sys
sys.argv.insert(1, "bowtie2-align")
runpy.run_path(str(Path(__file__).with_name("ngs_alignment.py")), run_name="__main__")
