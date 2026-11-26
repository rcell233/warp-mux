"""Observe the signed-out agy UI over two real zmx attach clients."""
import collections
import fcntl
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time

zmx, agy = sys.argv[1:]
replies = {b"\x1b[?u": b"\x1b[?5u", b"\x1b[c": b"\x1b[?6c",
           b"\x1b[?2026$p": b"\x1b[?2026;2$y", b"\x1b]11;?\x07":b"\x1b]11;rgb:0000/0000/0000\x07"}
pattern = re.compile(b"|".join(re.escape(q) for q in replies))
with tempfile.TemporaryDirectory(prefix="zmx-agy-") as td:
    env = {"ZMX_DIR":td+"/s","HOME":td,"SHELL":"/bin/bash","PATH":"/usr/bin:/bin","TERM":"xterm-256color","TERM_PROGRAM":"WarpTerminal","LANG":"C.UTF-8","BROWSER":"/bin/false"}
    clients = {}
    def attach(rows, cols, *cmd):
        master,slave=pty.openpty()
        fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack("HHHH",rows,cols,0,0))
        proc=subprocess.Popen([zmx,"attach","agy-probe",*cmd],env=env,stdin=slave,stdout=slave,stderr=slave,cwd=td)
        os.close(slave)
        clients[master]=(proc,bytearray())
        return master
    def observe(seconds):
        ends=time.monotonic()+seconds
        counts=collections.Counter()
        while time.monotonic()<ends:
            for fd in select.select(list(clients),[],[],.05)[0]:
                data=os.read(fd,65536)
                buf=clients[fd][1]
                old=len(buf)
                buf.extend(data)
                for match in pattern.finditer(buf,max(0,old-20)):
                    if match.end()>old:
                        counts[match.group()]+=1
                        os.write(fd,replies[match.group()])
        return dict(counts)
    def logstats():
        logs=b"".join(f.read_bytes() for f in Path(env["ZMX_DIR"],"logs").glob("*.log"))
        return logs.count(b"setting new leader"),re.findall(rb'resize rows=(\d+) cols=(\d+)',logs)
    try:
        large=attach(48,140,"/bin/bash","--noprofile","--norc")
        observe(.7)
        small=attach(24,80)
        observe(.7)
        os.write(large, os.fsencode(agy)+b"\r")
        startup=observe(5)
        print("startup queries:",startup,flush=True)
        assert any(b"Google OAuth" in buf or b"Select login method" in buf for _,buf in clients.values()),"expected signed-out UI"
        for fd in (small,large):
            os.write(fd,b"x")
            observe(.4)
            leaders,sizes=logstats()
            queries=observe(2)
            after,seq=logstats()
            frames=sum(buf.count(b"\x1b[2J") for _,buf in clients.values())
            print("after handoff / 2s idle:", "new leader changes",after-leaders,"resizes",seq[len(sizes):][:20],"queries",queries,"cumulative clear frames",frames,flush=True)
    finally:
        subprocess.run([zmx,"kill","agy-probe"],env=env,timeout=8,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        for fd,(proc,buf) in clients.items():
            proc.terminate()
            os.close(fd)
