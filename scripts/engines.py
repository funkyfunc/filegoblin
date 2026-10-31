"""Other document-to-markdown tools, run the same way the harness runs
filegoblin, so the gauntlet and olmOCR-bench can grade them too.

Each tool lives in its own virtual environment under $FILEGOBLIN_ALTS
(default ~/Library/Caches/fgv/alts/<engine>), never in the repo, and runs
scripts/alt_engine_worker.py there: one long-lived process per worker that
loads the tool's models once. A file that hangs past the timeout kills its
worker (and anything it spawned) and fails alone; the next file gets a fresh
one. These tools have no memory guard of their own, so a watchdog kills a
worker whose processes (the tool and anything it spawned, such as
llama-server) together hold more than $FILEGOBLIN_ALT_MEMORY_MB of resident
memory (default 8192); that file fails with "memory limit". Setup, once per
engine:

    uv venv -p 3.12 $FILEGOBLIN_ALTS/docling
    VIRTUAL_ENV=$FILEGOBLIN_ALTS/docling uv pip install docling

(see INSTALL below for each engine's packages). Model downloads go to
$FILEGOBLIN_ALTS/.cache, not the user's caches.
"""
import json
import os
import queue
import signal
import subprocess
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
MEMORY_MB = int(os.environ.get("FILEGOBLIN_ALT_MEMORY_MB", "8192"))

# Limits for every filegoblin the harness starts: the CLI's own ceiling and
# its PDF workers' (exit 5 and a "took more than N MB" error when crossed).
FILEGOBLIN_LIMITS = {"FILEGOBLIN_MEMORY_MB": "6144", "FILEGOBLIN_PDF_MEMORY_MB": "4096"}


def filegoblin_env():
    env = dict(os.environ)
    for k, v in FILEGOBLIN_LIMITS.items():
        env.setdefault(k, v)
    return env


def over_ceiling(exit_code, stderr):
    """A filegoblin run stopped by its memory ceiling: a finding to record,
    never a reason to raise the limit."""
    return exit_code == 5 or "MB of memory" in (stderr or "")
ALTS = os.path.expanduser(os.environ.get("FILEGOBLIN_ALTS", "~/Library/Caches/fgv/alts"))

OFFICE = {".pdf", ".docx", ".pptx", ".html", ".htm"}

# What each engine reads, how it is installed, and its licence (the part that
# matters to anyone embedding it). Versions are those measured in
# docs/COMPARISON.md.
ENGINES = {
    "pymupdf4llm": {
        "formats": {".pdf"},
        "install": "pymupdf4llm pymupdf pymupdf-layout rapidocr",
        "license": "AGPL-3.0 or Artifex commercial",
    },
    "markitdown": {
        "formats": OFFICE,
        "install": "'markitdown[all]'",
        "license": "MIT",
    },
    "docling": {
        "formats": OFFICE,
        "install": "docling",
        "license": "MIT (models: Apache-2.0 / MIT)",
    },
    "marker": {
        "formats": {".pdf"},
        "install": "'marker-pdf[full]', plus llama.cpp's llama-server ($FILEGOBLIN_ALTS/llamacpp)",
        "license": "Apache-2.0 code; model weights modified OpenRAIL-M (free under $5M revenue)",
    },
    "mineru": {
        "formats": OFFICE,
        "install": "'mineru[all]'",
        "license": "MinerU Open Source License (Apache-2.0 based, extra conditions)",
    },
}


def engine_env(name):
    env = dict(os.environ)
    cache = os.path.join(ALTS, ".cache")
    env.update({
        "HF_HOME": os.path.join(cache, "huggingface"),
        "XDG_CACHE_HOME": cache,
        "MODELSCOPE_CACHE": os.path.join(cache, "modelscope"),
        "TORCH_HOME": os.path.join(cache, "torch"),
        "TOKENIZERS_PARALLELISM": "false",
        "PYTHONUNBUFFERED": "1",
        "HF_HUB_DISABLE_TELEMETRY": "1",
        "DO_NOT_TRACK": "1",
    })
    if name == "marker":
        for root, _, files in os.walk(os.path.join(ALTS, "llamacpp")):
            if "llama-server" in files:
                env["LLAMA_CPP_BINARY"] = os.path.join(root, "llama-server")
                break
    return env


def supports(name, path):
    return os.path.splitext(path)[1].lower() in ENGINES[name]["formats"]


class Worker:
    """One alt_engine_worker.py process."""

    def __init__(self, name, log):
        python = os.path.join(ALTS, name, "bin", "python")
        if not os.path.exists(python):
            raise SystemExit(f"{name} is not installed: no {python} (see scripts/engines.py)")
        self.proc = subprocess.Popen(
            [python, os.path.join(HERE, "alt_engine_worker.py"), name],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log,
            text=True, env=engine_env(name), start_new_session=True)
        self.lines = queue.Queue()
        threading.Thread(target=self._pump, daemon=True).start()
        self.load = None

    def _pump(self):
        for line in self.proc.stdout:
            self.lines.put(line)
        self.lines.put(None)

    def _reply(self, timeout):
        deadline = time.time() + timeout
        while True:
            try:
                line = self.lines.get(timeout=min(0.5, max(0.01, deadline - time.time())))
                break
            except queue.Empty:
                if time.time() >= deadline:
                    raise
                if self.rss_mb() > MEMORY_MB:
                    raise MemoryError(f"memory limit: over {MEMORY_MB} MB resident")
        if line is None:
            raise EOFError(f"worker exited ({self.proc.wait()})")
        return json.loads(line)

    def rss_mb(self):
        """Resident memory of the worker's process group, in MB."""
        out = subprocess.run(["ps", "-axo", "pgid=,rss="], capture_output=True, text=True).stdout
        kb = sum(int(r) for g, r in (l.split() for l in out.splitlines() if l.strip())
                 if int(g) == self.proc.pid)
        return kb // 1024

    def ready(self, timeout):
        self.load = self._reply(timeout)["load"]

    def convert(self, src, out, timeout):
        self.proc.stdin.write(json.dumps({"src": src, "out": out}) + "\n")
        self.proc.stdin.flush()
        return self._reply(timeout)

    def kill(self):
        try:
            os.killpg(self.proc.pid, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            pass
        self.proc.wait()


def convert_all(name, jobs, timeout=900, load_timeout=1800):
    """Returns a function `convert(src, out) -> {"ok", "error", "elapsed",
    "load"}` that runs `name` with at most `jobs` workers at once, and a
    `close()` for the end of the run."""
    os.makedirs(os.path.join(ALTS, "logs"), exist_ok=True)
    log = open(os.path.join(ALTS, "logs", f"{name}.log"), "a")
    idle, lock = [], threading.Lock()
    slots = threading.Semaphore(max(1, jobs))

    def convert(src, out):
        with slots:
            with lock:
                worker = idle.pop() if idle else None
            first = worker is None
            try:
                if worker is None:
                    worker = Worker(name, log)
                    worker.ready(load_timeout)
                reply = worker.convert(src, out, timeout)
            except queue.Empty:
                worker.kill()
                return {"ok": False, "error": f"timeout after {timeout}s", "elapsed": timeout, "load": 0.0}
            except MemoryError as e:
                worker.kill()
                return {"ok": False, "error": str(e), "elapsed": 0.0, "load": 0.0}
            except (EOFError, OSError, ValueError) as e:
                if worker is not None:
                    worker.kill()
                return {"ok": False, "error": f"worker died: {e}", "elapsed": 0.0, "load": 0.0}
            reply["load"] = worker.load if first else 0.0
            with lock:
                idle.append(worker)
            return reply

    def close():
        for w in idle:
            w.proc.stdin.close()
            try:
                w.proc.wait(30)
            except subprocess.TimeoutExpired:
                w.kill()
        log.close()

    return convert, close

