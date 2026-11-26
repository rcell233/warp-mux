"""Exercise zmx leader changes using two real attach clients and their real PTYs."""
import fcntl
import os
from pathlib import Path
import pty
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
import tty

if sys.argv[1] == "child":
    tty.setraw(0)
    def report(*args):
        size = os.get_terminal_size()
        with open(sys.argv[2], "a") as stream:
            stream.write(f"{size.columns}x{size.lines}\n")
        os.write(1, f"probe:{size.columns}\x1b[?u".encode())
    signal.signal(signal.SIGWINCH, report)
    report()
    while os.read(0, 512):
        pass
    sys.exit(0)

binary, version = sys.argv[1:]
with tempfile.TemporaryDirectory(prefix="wmux-") as directory:
    env = dict(os.environ, ZMX_DIR=directory, SHELL="/bin/sh", TERM="xterm-256color")
    trace = Path(directory, "sizes")
    clients = []
    def attach(rows, cols, command=()):
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        proc = subprocess.Popen([binary, "attach", "resize", *command], env=env,
                                stdin=slave, stdout=slave, stderr=slave)
        os.close(slave)
        clients.append((proc, master))
        return master
    def sizes():
        return trace.read_text().splitlines() if trace.exists() else []
    def until(predicate, msg):
        end = time.monotonic() + 5
        while not predicate():
            if time.monotonic() > end:
                raise RuntimeError(f"{msg}: {sizes()[-10:]}")
            time.sleep(.02)
    def handoff(fd, expected):
        end = time.monotonic() + 7
        while True:
            os.write(fd, b"x")
            time.sleep(.1)
            if sizes()[-1:] == [expected]:
                return
            if time.monotonic() >= end:
                raise RuntimeError(f"keypress did not claim {expected}: {sizes()[-10:]}")
    try:
        large = attach(48, 140, [sys.executable, str(Path(__file__).resolve()), "child", str(trace)])
        until(lambda: sizes()[-1:] == ["140x48"], "initial size")
        small = attach(24, 80)
        until(lambda: "clients=2" in subprocess.check_output([binary,"list"],env=env,text=True), "two clients")
        handoff(small, "80x24")
        handoff(large, "140x48")
        # Deliver only the final query reply from the passive client. No keys or resizes follow.
        for _, fd in clients:
            while select.select([fd], [], [], 0)[0]:
                os.read(fd, 65536)
        start = len(sizes())
        os.write(small, b"\x1b[?5u")
        tail = {}
        deadline = time.monotonic()+1.5
        while time.monotonic()<deadline and len(sizes())-start < 20:
            for fd in select.select([large,small],[],[],.05)[0]:
                data = tail.get(fd,b"") + os.read(fd,65536)
                # Exercise the reply from the non-leader in isolation.
                query = b"probe:140\x1b[?u" if fd == small else b"probe:80\x1b[?u"
                count = data.count(query)
                if count: os.write(fd, b"\x1b[?5u"*count)
                tail[fd] = data[-(len(query)-1):]
        changes = sizes()[start:]
        print(version, "idle size changes:", len(changes), changes[:10],flush=True)
        if version == "0.8.1": assert {"80x24","140x48"}.issubset(changes)
        else: assert not changes, changes
        handoff(small, "80x24")
        handoff(large, "140x48")
        print(version, "explicit keyboard handoff in both directions OK")
    finally:
        subprocess.run([binary,"kill","resize"],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=5)
        for proc, fd in clients:
            proc.terminate()
            os.close(fd)
