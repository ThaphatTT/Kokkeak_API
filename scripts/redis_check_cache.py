"""Minimal Redis SCAN+DEL helper — bypasses HELLO handshake for managed Redis.

Usage:
    python scripts/redis_check_cache.py           # scan only
    python scripts/redis_check_cache.py --delete  # scan + delete old-format keys
"""

import socket
import sys

HOST = "10.0.200.83"
PORT = 6379


class Redis:
    """Tiny RESP2 client. No PING/HELLO handshake; managed Redis often blocks HELLO."""

    def __init__(self, host, port):
        self.s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.s.settimeout(10)
        self.s.connect((host, port))
        self.buf = b""

    def _fill(self):
        chunk = self.s.recv(4096)
        if not chunk:
            raise ConnectionError("closed")
        self.buf += chunk

    def _recv_line(self):
        while b"\r\n" not in self.buf:
            self._fill()
        line, self.buf = self.buf.split(b"\r\n", 1)
        return line

    def _recv_n(self, n):
        while len(self.buf) < n + 2:  # +2 for trailing \r\n
            self._fill()
        out, self.buf = self.buf[:n], self.buf[n + 2 :]
        return out

    def _read_one(self):
        head = self._recv_line()
        if head == b"$-1":
            return None
        if head.startswith(b"$"):
            return self._recv_n(int(head[1:])).decode(errors="replace")
        if head.startswith(b":"):
            return int(head[1:])
        if head.startswith(b"*"):
            n = int(head[1:])
            return [self._read_one() for _ in range(n)]
        if head.startswith(b"+"):
            return head[1:].decode(errors="replace")
        return head.decode(errors="replace")

    def cmd(self, *args):
        out = f"*{len(args)}\r\n".encode()
        for a in args:
            a = str(a)
            out += f"${len(a)}\r\n{a}\r\n".encode()
        self.s.sendall(out)
        return self._read_one()

    def scan_iter(self, match, count=100):
        cursor = 0
        while True:
            cursor, keys = self._scan(cursor, match, count)
            for k in keys or []:
                yield k
            if cursor == 0:
                break

    def _scan(self, cursor, match, count):
        out = self.cmd("SCAN", cursor, "MATCH", match, "COUNT", count)
        return int(out[0]), out[1] or []

    def get(self, key):
        return self.cmd("GET", key)

    def ttl(self, key):
        return self.cmd("TTL", key)

    def delete(self, *keys):
        return self.cmd("DEL", *keys)


def dump_keys(r, label, match):
    keys = list(r.scan_iter(match, 100))
    print(f"\n[{label}] {match}")
    print(f"  keys found: {len(keys)}")
    for k in keys[:10]:
        print(f"  - {k}  ttl={r.ttl(k)}s  val={r.get(k)}")
    if len(keys) > 10:
        print(f"  ... and {len(keys) - 10} more")
    return keys


r = Redis(HOST, PORT)
print(f"PING -> {r.cmd('PING')}")
print(f"=== Connected to Redis {HOST}:{PORT} ===")

old_keys = dump_keys(r, "OLD format", "kokkak:v1:perm:*:PAGE_PERMISSIONS_VIEW")
new_keys = dump_keys(r, "NEW format", "kokkak:v1:perm:*:PERMISSIONS_VIEW")
all_keys = list(r.scan_iter("kokkak:v1:perm:*", 100))
print(f"\n[TOTAL] kokkak:v1:perm:*  ->  {len(all_keys)} keys")

if "--delete" in sys.argv:
    if old_keys:
        # DEL with varargs; chunked in groups of 500 to avoid huge commands
        deleted = 0
        for i in range(0, len(old_keys), 500):
            deleted += r.delete(*old_keys[i : i + 500])
        print(f"\n>>> Deleted {deleted} old-format keys.")
    else:
        print("\n>>> Nothing to delete.")
else:
    if old_keys:
        print(f"\n>>> Re-run with --delete to drop {len(old_keys)} old keys.")
    else:
        print("\n>>> Cache is clean — no stale old-format keys.")
