"""``filegoblin`` console entry point: runs the bundled native binary."""

import os
import stat
import subprocess
import sys

from . import binary_path, environment


def main() -> None:
    exe = binary_path()
    if not os.path.isfile(exe):
        sys.stderr.write(
            "filegoblin: the native binary is missing from this installation (%s).\n"
            "Reinstall with `pip install --force-reinstall filegoblin`.\n" % exe
        )
        sys.exit(1)
    if os.name != "nt" and not os.access(exe, os.X_OK):
        # Some installers drop the executable bit; restore it once.
        try:
            os.chmod(exe, os.stat(exe).st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
        except OSError:
            pass
    argv = [exe] + sys.argv[1:]
    env = environment()
    if os.name == "nt":
        sys.exit(subprocess.call(argv, env=env))
    os.execve(exe, argv, env)


if __name__ == "__main__":
    main()
