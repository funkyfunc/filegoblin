"""PDF vision models for filegoblin: PP-DocLayoutV3 (layout and reading
order), pix2text MFR (formulas to LaTeX), Table Transformer (tables) and
PP-OCRv6 (scan OCR). Licenses are in ``data/LICENSES.md``.

The weights arrive through the ``filegoblin-model-*`` packages this one
depends on, one per model (``filegoblin-model-layout-1``,
``filegoblin-model-ocr-recognizer``, …). PyPI limits the size of a single
file, so the two largest models come in halves, which filegoblin joins when
it loads them.
"""

from pathlib import Path

__all__ = ["paths"]


def paths() -> list:
    """Directories filegoblin should search for model files."""
    data = Path(__file__).resolve().parent / "data"
    return [str(data)] if data.is_dir() else []
