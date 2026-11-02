import json
import os
import signal
import stat
import sys
import tempfile

clipboard = sys.argv[1] == "--clipboard"
if clipboard:
    _, extension, expected = sys.argv[1:]
    if extension not in ("png", "jpg", "gif", "webp"):
        raise ValueError("Unsupported clipboard image format")
    cache = os.environ.get("XDG_CACHE_HOME", "")
    if not os.path.isabs(cache):
        cache = os.path.join(os.path.expanduser("~"), ".cache")
    base = os.path.join(cache, "warp-mux", "paste")
    os.makedirs(base, mode=0o700, exist_ok=True)
    directory = tempfile.mkdtemp(prefix="image-", dir=base)
    name = "image." + extension
else:
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
    destination = os.path.join(directory, name)
    os.replace(staged, destination)
    print("__WARP_MUX_TRANSFER__" + str(transferred), flush=True)
    if clipboard:
        print("__WARP_MUX_FILE__" + json.dumps(destination), flush=True)
finally:
    if os.path.exists(staged):
        os.unlink(staged)
    if clipboard and not os.listdir(directory):
        os.rmdir(directory)
