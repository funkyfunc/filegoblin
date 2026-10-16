"""PDF vision models for filegoblin: PP-DocLayoutV3 (layout and reading
order), pix2text MFR (formulas to LaTeX), Table Transformer (tables) and
PP-OCRv6 (scan OCR). Licenses are in ``data/LICENSES.md``.

The weights are stored in pieces under ``data/`` (PyPI limits the size of a
single file); filegoblin joins them when it loads a model. The pieces arrive
through the ``filegoblin-models-part*`` packages this one depends on.
"""

from pathlib import Path

__all__ = ["paths"]


def paths() -> list:
    """Directories filegoblin should search for model files."""
    data = Path(__file__).resolve().parent / "data"
    return [str(data)] if data.is_dir() else []
