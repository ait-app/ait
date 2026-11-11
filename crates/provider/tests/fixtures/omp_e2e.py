#!/usr/bin/env python3
"""Offline OMP RPC peer for browser consumer tests.

Delegates to the Rust test fixture with an isolated agent directory beside this
launcher, so specs never read or write the developer's OMP configuration.
"""
import os
from pathlib import Path
import runpy
import sys

here = Path(sys.argv[0]).resolve().parent
os.environ['PI_CODING_AGENT_DIR'] = str(here / 'omp-agent')
runpy.run_path(str(here / 'omp.py'), run_name='__main__')