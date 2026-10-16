#!/usr/bin/env python3
"""Builds the PyPI wheels and npm packages for a filegoblin release.

Inputs are what the release workflow already has: dist's per-platform
archives (`filegoblin-<target>.tar.xz`, `.zip` on Windows) and the vision
model directory (or the `filegoblin-vision-models.tar.gz` archive).

PyPI (stdlib-built wheels, no build backend needed):
    filegoblin                   one wheel per platform: the native binary and
                                 a small launcher; extra `models` pulls in…
    filegoblin-models            the model package (launcher-visible paths),
                                 which depends on…
    filegoblin-models-part1..N   the weights, in pieces under 90 MiB each

npm:
    filegoblin                   the launcher (npm/), with optional
                                 dependencies on…
    filegoblin-<os>-<cpu>        one package per platform with the binary
    filegoblin-models            which depends on…
    filegoblin-models-part1..N   the weights, in pieces

Both registries limit file sizes (PyPI: 100 MB per file by default), and
company package mirrors often do too, so the weights are split into pieces
named `<model>.part<i>of<n>`; filegoblin joins them when it loads a model.

usage:
    package_registries.py pypi --version V --archives DIR --models DIR|ARCHIVE --out DIR
    package_registries.py npm  --version V --archives DIR --models DIR|ARCHIVE --out DIR
"""

import argparse
import base64
import hashlib
import io
import json
import os
import re
import shutil
import stat
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
PIECE = 90 * 1024 * 1024  # largest piece of one model file
PART_MAX = 95 * 1024 * 1024  # largest model part package (before compression)

# The model files a release ships, and the license files that go with them.
MODEL_FILES = [
    "pp-doclayout-v3.onnx",
    "mfr-encoder.onnx",
    "mfr-decoder-init.onnx",
    "mfr-decoder-step.onnx",
    "mfr-tokenizer.json",
    "tatr-structure.onnx",
    "ppocr-v6m-det.onnx",
    "ppocr-v6m-rec.onnx",
    "ppocr-v6-dict.txt",
]
LICENSE_FILES = ["LICENSES.md", "LICENSE-APACHE-2.0.txt", "LICENSE-MIT.txt"]

# dist target -> (PyPI platform tag or None for "compute from the binary",
#                 npm os, npm cpu, binary name)
TARGETS = {
    "aarch64-apple-darwin": ("macosx_11_0_arm64", "darwin", "arm64", "filegoblin"),
    "x86_64-apple-darwin": ("macosx_10_12_x86_64", "darwin", "x64", "filegoblin"),
    "x86_64-unknown-linux-gnu": (None, "linux", "x64", "filegoblin"),
    "aarch64-unknown-linux-gnu": (None, "linux", "arm64", "filegoblin"),
    "x86_64-pc-windows-msvc": ("win_amd64", "win32", "x64", "filegoblin.exe"),
}

SUMMARY = "Converts PDFs, Office documents, web pages and more into clean Markdown for LLMs"
HOMEPAGE = "https://github.com/funkyfunc/filegoblin"


# --------------------------------------------------------------------------
# Inputs


def find_archives(directory: Path) -> dict:
    """{target: archive path} for every dist archive in `directory`."""
    found = {}
    for target in TARGETS:
        for ext in (".tar.xz", ".tar.gz", ".zip"):
            p = directory / f"filegoblin-{target}{ext}"
            if p.is_file():
                found[target] = p
                break
    return found


def extract_binary(archive: Path, name: str) -> bytes:
    """The `filegoblin` executable inside a dist archive."""
    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as z:
            for info in z.infolist():
                if info.filename.rsplit("/", 1)[-1] == name:
                    return z.read(info)
    else:
        with tarfile.open(archive) as t:
            for m in t.getmembers():
                if m.isfile() and m.name.rsplit("/", 1)[-1] == name:
                    return t.extractfile(m).read()
    raise SystemExit(f"{archive}: no {name} inside")


def model_dir(source: Path, scratch: Path) -> Path:
    """The model directory, unpacking the release archive if given one."""
    if source.is_dir():
        return source
    out = scratch / "models-unpacked"
    with tarfile.open(source) as t:
        t.extractall(out, filter="data")
    inner = out / "models"
    return inner if inner.is_dir() else out


def plan_pieces(models: Path) -> list:
    """[(piece file name, source path, offset, length)], every model file
    split into pieces of at most PIECE bytes (small files stay whole)."""
    pieces = []
    for name in MODEL_FILES + LICENSE_FILES:
        p = models / name
        if not p.is_file():
            if name in LICENSE_FILES:
                continue
            raise SystemExit(f"model file missing: {p}")
        size = p.stat().st_size
        n = max(1, -(-size // PIECE))
        if n == 1:
            pieces.append((name, p, 0, size))
        else:
            for i in range(n):
                off = i * PIECE
                pieces.append((f"{name}.part{i + 1}of{n}", p, off, min(PIECE, size - off)))
    return pieces


def pack_parts(pieces: list) -> list:
    """Groups pieces into parts of at most PART_MAX bytes (first fit,
    largest first), each part a list of pieces."""
    parts = []
    for piece in sorted(pieces, key=lambda x: -x[3]):
        for part in parts:
            if sum(x[3] for x in part) + piece[3] <= PART_MAX:
                part.append(piece)
                break
        else:
            parts.append([piece])
    for part in parts:
        part.sort(key=lambda x: x[0])
    return parts


def read_piece(piece) -> bytes:
    _, path, off, length = piece
    with open(path, "rb") as f:
        f.seek(off)
        return f.read(length)


def glibc_tag(binary: bytes, arch: str) -> str:
    """manylinux tag for the newest glibc symbol version the binary needs."""
    versions = [tuple(map(int, m)) for m in re.findall(rb"GLIBC_(\d+)\.(\d+)", binary)]
    major, minor = max(versions) if versions else (2, 17)
    return f"manylinux_{major}_{minor}_{arch}"


# --------------------------------------------------------------------------
# PyPI


class Wheel:
    """A wheel written with zipfile, RECORD included."""

    def __init__(self, out: Path, dist: str, version: str, tag: str):
        self.dist = dist.replace("-", "_")
        self.version = version
        self.tag = tag
        self.path = out / f"{self.dist}-{version}-{tag}.whl"
        self.zip = zipfile.ZipFile(self.path, "w", zipfile.ZIP_DEFLATED, compresslevel=6)
        self.records = []

    def add(self, arcname: str, data: bytes, executable: bool = False):
        info = zipfile.ZipInfo(arcname, date_time=(2020, 1, 1, 0, 0, 0))
        info.compress_type = zipfile.ZIP_DEFLATED
        mode = 0o755 if executable else 0o644
        info.external_attr = (stat.S_IFREG | mode) << 16
        self.zip.writestr(info, data)
        digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
        self.records.append(f"{arcname},sha256={digest},{len(data)}")

    def finish(self, metadata: str, entry_points: str = None, purelib: bool = True):
        info_dir = f"{self.dist}-{self.version}.dist-info"
        self.add(f"{info_dir}/METADATA", metadata.encode())
        wheel = (
            "Wheel-Version: 1.0\n"
            "Generator: filegoblin package_registries.py\n"
            f"Root-Is-Purelib: {'true' if purelib else 'false'}\n"
            f"Tag: {self.tag}\n"
        )
        self.add(f"{info_dir}/WHEEL", wheel.encode())
        if entry_points:
            self.add(f"{info_dir}/entry_points.txt", entry_points.encode())
        self.add(f"{info_dir}/licenses/LICENSE", (REPO / "LICENSE").read_bytes())
        record = "\n".join(self.records + [f"{info_dir}/RECORD,,"]) + "\n"
        info = zipfile.ZipInfo(f"{info_dir}/RECORD", date_time=(2020, 1, 1, 0, 0, 0))
        info.external_attr = (stat.S_IFREG | 0o644) << 16
        self.zip.writestr(info, record)
        self.zip.close()
        return self.path


def metadata(name: str, version: str, summary: str, requires=(), extras=(), readme: str = "") -> str:
    lines = [
        "Metadata-Version: 2.4",
        f"Name: {name}",
        f"Version: {version}",
        f"Summary: {summary}",
        f"Home-page: {HOMEPAGE}",
        "License-Expression: MIT",
        "License-File: LICENSE",
        f"Project-URL: Source, {HOMEPAGE}",
        "Requires-Python: >=3.8",
    ]
    lines += [f"Provides-Extra: {e}" for e in extras]
    lines += [f"Requires-Dist: {r}" for r in requires]
    if readme:
        lines += ["Description-Content-Type: text/markdown", "", readme]
    return "\n".join(lines) + "\n"


def build_pypi(version: str, archives: dict, models: Path, out: Path) -> list:
    out.mkdir(parents=True, exist_ok=True)
    built = []
    pkg = REPO / "packaging" / "python"
    readme = (pkg / "README.md").read_text()
    launcher = {
        "filegoblin/__init__.py": (pkg / "filegoblin" / "__init__.py").read_bytes(),
        "filegoblin/__main__.py": (pkg / "filegoblin" / "__main__.py").read_bytes(),
    }
    for target, archive in sorted(archives.items()):
        plat, _, cpu, binname = TARGETS[target]
        binary = extract_binary(archive, binname)
        if plat is None:
            plat = glibc_tag(binary, "x86_64" if cpu == "x64" else "aarch64")
        w = Wheel(out, "filegoblin", version, f"py3-none-{plat}")
        for arc, data in launcher.items():
            w.add(arc, data)
        w.add(f"filegoblin/bin/{binname}", binary, executable=True)
        built.append(
            w.finish(
                metadata(
                    "filegoblin",
                    version,
                    SUMMARY,
                    requires=[f'filegoblin-models=={version}; extra == "models"'],
                    extras=["models"],
                    readme=readme,
                ),
                entry_points="[console_scripts]\nfilegoblin = filegoblin.__main__:main\n",
                purelib=False,
            )
        )
    if models is not None:
        parts = pack_parts(plan_pieces(models))
        for i, part in enumerate(parts, 1):
            w = Wheel(out, f"filegoblin-models-part{i}", version, "py3-none-any")
            for piece in part:
                w.add(f"filegoblin_models/data/{piece[0]}", read_piece(piece))
            built.append(
                w.finish(
                    metadata(
                        f"filegoblin-models-part{i}",
                        version,
                        f"filegoblin PDF vision model weights, part {i} of {len(parts)} "
                        "(install filegoblin[models] instead)",
                    )
                )
            )
        w = Wheel(out, "filegoblin-models", version, "py3-none-any")
        w.add(
            "filegoblin_models/__init__.py",
            (pkg / "filegoblin_models" / "__init__.py").read_bytes(),
        )
        built.append(
            w.finish(
                metadata(
                    "filegoblin-models",
                    version,
                    "PDF vision models for filegoblin (layout, formulas, tables, scan OCR)",
                    requires=[f"filegoblin-models-part{i}=={version}" for i in range(1, len(parts) + 1)],
                    readme="PDF vision models for [filegoblin](https://github.com/funkyfunc/filegoblin). "
                    'Install with `pip install "filegoblin[models]"`. '
                    "Model licenses (Apache-2.0 and MIT) are in `filegoblin_models/data/LICENSES.md`.\n",
                )
            )
        )
    return built


# --------------------------------------------------------------------------
# npm


def write_json(path: Path, data: dict):
    path.write_text(json.dumps(data, indent=2) + "\n")


def npm_common(version: str) -> dict:
    return {
        "version": version,
        "license": "MIT",
        "repository": {"type": "git", "url": "git+https://github.com/funkyfunc/filegoblin.git"},
        "homepage": f"{HOMEPAGE}#readme",
    }


def build_npm(version: str, archives: dict, models: Path, out: Path) -> list:
    """Package directories, ready for `npm publish <dir>`, in publish order
    (dependencies before the packages that need them)."""
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    dirs = []
    platform_names = []
    for target, archive in sorted(archives.items()):
        _, os_name, cpu, binname = TARGETS[target]
        name = f"filegoblin-{os_name}-{cpu}"
        d = out / name
        (d / "bin").mkdir(parents=True)
        b = d / "bin" / binname
        b.write_bytes(extract_binary(archive, binname))
        b.chmod(0o755)
        shutil.copy(REPO / "LICENSE", d / "LICENSE")
        write_json(
            d / "package.json",
            {
                "name": name,
                **npm_common(version),
                "description": f"The filegoblin binary for {os_name}-{cpu} (install `filegoblin` instead)",
                "os": [os_name],
                "cpu": [cpu],
                "files": ["bin", "LICENSE"],
                "preferUnplugged": True,
            },
        )
        dirs.append(d)
        platform_names.append(name)

    if models is not None:
        parts = pack_parts(plan_pieces(models))
        part_names = []
        for i, part in enumerate(parts, 1):
            name = f"filegoblin-models-part{i}"
            d = out / name
            (d / "models").mkdir(parents=True)
            for piece in part:
                (d / "models" / piece[0]).write_bytes(read_piece(piece))
            shutil.copy(REPO / "LICENSE", d / "LICENSE")
            write_json(
                d / "package.json",
                {
                    "name": name,
                    **npm_common(version),
                    "description": f"filegoblin PDF vision model weights, part {i} of {len(parts)} "
                    "(install `filegoblin-models` instead)",
                    "files": ["models", "LICENSE"],
                },
            )
            dirs.append(d)
            part_names.append(name)
        d = out / "filegoblin-models"
        d.mkdir()
        shutil.copy(REPO / "packaging" / "npm-models" / "index.js", d / "index.js")
        write_json(d / "parts.json", part_names)
        shutil.copy(REPO / "LICENSE", d / "LICENSE")
        (d / "README.md").write_text(
            "# filegoblin-models\n\nPDF vision models for "
            "[filegoblin](https://github.com/funkyfunc/filegoblin): layout and reading order, "
            "formulas as LaTeX, tables, and OCR for scanned pages.\n\n"
            "```bash\nnpm install -g filegoblin filegoblin-models\n```\n\n"
            "The `filegoblin` launcher finds this package next to it and uses the models. "
            "Model licenses (Apache-2.0 and MIT) are in the `models/LICENSES.md` file of the part packages.\n"
        )
        write_json(
            d / "package.json",
            {
                "name": "filegoblin-models",
                **npm_common(version),
                "description": "PDF vision models for filegoblin (layout, formulas, tables, scan OCR)",
                "main": "index.js",
                "files": ["index.js", "parts.json", "README.md", "LICENSE"],
                "dependencies": {n: version for n in part_names},
            },
        )
        dirs.append(d)

    d = out / "filegoblin"
    shutil.copytree(REPO / "npm", d)
    pkg = json.loads((d / "package.json").read_text())
    pkg["version"] = version
    pkg["optionalDependencies"] = {n: version for n in platform_names}
    write_json(d / "package.json", pkg)
    dirs.append(d)
    return dirs


# --------------------------------------------------------------------------


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("registry", choices=["pypi", "npm"])
    ap.add_argument("--version", required=True)
    ap.add_argument("--archives", type=Path, required=True, help="directory with dist's archives")
    ap.add_argument("--models", type=Path, help="model directory or filegoblin-vision-models.tar.gz")
    ap.add_argument("--out", type=Path, required=True)
    args = ap.parse_args()

    version = args.version.lstrip("v")
    archives = find_archives(args.archives)
    if not archives:
        raise SystemExit(f"no filegoblin-<target> archives in {args.archives}")
    missing = sorted(set(TARGETS) - set(archives))
    if missing:
        print(f"warning: no archive for {', '.join(missing)}", file=sys.stderr)
    with tempfile.TemporaryDirectory() as scratch:
        models = model_dir(args.models, Path(scratch)) if args.models else None
        if args.registry == "pypi":
            for p in build_pypi(version, archives, models, args.out):
                print(f"{p}  {p.stat().st_size / 1e6:.1f} MB")
        else:
            for d in build_npm(version, archives, models, args.out):
                size = sum(f.stat().st_size for f in d.rglob("*") if f.is_file())
                print(f"{d}  {size / 1e6:.1f} MB")


if __name__ == "__main__":
    main()
