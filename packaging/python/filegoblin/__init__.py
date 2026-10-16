"""filegoblin: converts PDFs, Office documents, web pages and more into
clean Markdown for LLMs.

This package ships the native `filegoblin` binary for your platform and a
small launcher. Install ``filegoblin[models]`` to add the PDF vision models
(layout, formulas, tables, scan OCR); the launcher points the binary at them.
"""

import os
import sys
from pathlib import Path

__all__ = ["binary_path", "model_dirs"]


def binary_path() -> str:
    """Path of the bundled filegoblin executable."""
    name = "filegoblin.exe" if sys.platform == "win32" else "filegoblin"
    return str(Path(__file__).resolve().parent / "bin" / name)


def model_dirs() -> list:
    """Directories holding the PDF vision models from ``filegoblin-models``,
    or an empty list when that package is not installed."""
    try:
        import filegoblin_models
    except ImportError:
        return []
    return list(filegoblin_models.paths())


def environment(base=None) -> dict:
    """``base`` (default: the current environment) with FILEGOBLIN_MODELS
    extended by the installed model package. A directory the user already set
    there comes first."""
    env = dict(os.environ if base is None else base)
    dirs = model_dirs()
    if dirs:
        existing = env.get("FILEGOBLIN_MODELS")
        env["FILEGOBLIN_MODELS"] = os.pathsep.join(([existing] if existing else []) + dirs)
    return env
