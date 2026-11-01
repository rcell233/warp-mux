import os
import signal
import stat
import sys
import tempfile

base, directory, name, expected = sys.argv[1:]
base = os.path.realpath(os.path.expanduser(base))
directory = os.path.realpath(directory)
expected = int(expected)
if os.path.commonpath([base, directory]) != base:
    raise ValueError("Upload directory is outside the base path")
if not name or name in (".", "..") or "/" in name or any(ord(c) < 32 or ord(c) == 127 for c in name):
    raise ValueError("Invalid upload filename")
if not stat.S_ISDIR(os.stat(directory).st_mode):
    raise ValueError("Upload destination must be a directory")

fd, staged = tempfile.mkstemp(prefix=".warp-mux-upload-", dir=directory)


def interrupted(signum, frame):
    raise InterruptedError("Upload connection closed")


signal.signal(signal.SIGHUP, interrupted)
signal.signal(signal.SIGTERM, interrupted)
try:
    transferred = 0
    last_report = 0
    with os.fdopen(fd, "wb") as output:
        while True:
            chunk = sys.stdin.buffer.read(256 * 1024)
            if not chunk:
                break
            output.write(chunk)
            transferred += len(chunk)
            if transferred - last_report >= 1024 * 1024:
                print("__WARP_MUX_TRANSFER__" + str(transferred), flush=True)
                last_report = transferred
        if transferred != expected:
            raise ValueError("Source changed or upload was interrupted")
        output.flush()
        os.fsync(output.fileno())
    os.replace(staged, os.path.join(directory, name))
    print("__WARP_MUX_TRANSFER__" + str(transferred), flush=True)
finally:
    if os.path.exists(staged):
        os.unlink(staged)
