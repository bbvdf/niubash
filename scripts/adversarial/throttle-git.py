#!/usr/bin/env python3
"""throttle-git.py -- a local HTTP proxy that slows, stalls or blackholes
git traffic on purpose.

Simulates the timing/hostility axis (adversarial-matrix lane AM09) and the
captive-portal corner of the hostile-network axis (AM01): a network that is
UP but hostile -- 10 KB/s clones (a phone hotspot, a congested link), stalls
after the first bytes (a captive portal / a dying middlebox), and
blackholes (connect succeeds, then silence forever -- the worst hang
class, the one the "startup must stay bounded" assertions exist for).
niu's git traffic goes through this proxy via
`-c http.proxy=http://127.0.0.1:PORT` (or https_proxy in the sandbox env),
so probes assert the PRODUCT's behavior on a slow network: bounded
startup, visible progress, honest failure, no silent partial state.

Forwarding styles (both handled):
  CONNECT host:port   https tunnels (the git->github.com shape)
  absolute-form GET   plain http without CONNECT (local http fixtures)

Knobs (composable):
  --mode blackhole     accept, then forward NOTHING and hold (default hold
                       is bounded by --hold-seconds so harnesses cannot
                       wedge; a client without its own timeout hangs first)
  --mode normal        clean unlimited tunnel (baseline control)
  --rate-kb-per-s N    token-bucket throttle, N KB/s (default 10)
  --latency-ms N       extra delay before the first byte each direction
  --stall-after-bytes  forward N bytes, then hold and forward nothing
                       (captive portal with partial data)

Deterministic, offline, no admin, stdlib only.

Usage (lane probe example -- 10 KB/s clone):
    python scripts/adversarial/throttle-git.py --port 8889 \
        --rate-kb-per-s 10 --log-file throttle.log &
    git -c http.proxy=http://127.0.0.1:8889 clone http://127.0.0.1:8099/r.git
    kill %1

Exit codes: 0 on clean Ctrl-C shutdown; 2 on usage error.
"""

from __future__ import annotations

import argparse
import select
import socket
import struct
import sys
import threading
import time
from urllib.parse import urlsplit

LOG_LOCK = threading.Lock()


def log(msg: str, log_file) -> None:
    line = f"[{time.strftime('%H:%M:%S')}] {msg}"
    with LOG_LOCK:
        print(line, flush=True)
        if log_file:
            with open(log_file, "a", encoding="utf-8") as fh:
                fh.write(line + "\n")


def kill_sock(sock: socket.socket) -> None:
    try:
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0))
    except OSError:
        pass
    try:
        sock.close()
    except OSError:
        pass


def graceful_close(sock: socket.socket) -> None:
    """Close WITHOUT the linger-0 RST: drain briefly, then FIN. A clean
    close must stay clean -- an RST racing the client's next pipelined
    request would fake a network failure the lane did not ask for."""
    try:
        sock.settimeout(0.3)
        deadline = time.monotonic() + 1.0
        while time.monotonic() < deadline:
            if not sock.recv(65536):
                break
    except (OSError, socket.timeout):
        pass
    try:
        sock.shutdown(socket.SHUT_RDWR)
    except OSError:
        pass
    try:
        sock.close()
    except OSError:
        pass


def read_head(sock: socket.socket) -> bytes:
    data = b""
    while b"\r\n\r\n" not in data and len(data) < 65536:
        chunk = sock.recv(8192)
        if not chunk:
            break
        data += chunk
    return data


def to_origin_form(head: bytes, path: str) -> bytes:
    """Rewrite an absolute-form request line (`GET http://h/p HTTP/1.1`) to
    origin-form (`GET /p HTTP/1.1`) and drop hop-by-hop proxy headers --
    plain HTTP servers (python http.server included) reject absolute-form."""
    lines = head.split(b"\r\n")
    parts = lines[0].split()
    method = parts[0] if parts else b"GET"
    version = parts[2] if len(parts) >= 3 else b"HTTP/1.1"
    first = method + b" " + path.encode() + b" " + version
    kept = [first]
    for line in lines[1:]:
        if not line:
            continue
        low = line.lower()
        if low.startswith(b"proxy-connection:") or low.startswith(b"proxy-authorization:"):
            continue
        kept.append(line)
    return b"\r\n".join(kept) + b"\r\n\r\n"


class LeakyLimiter:
    """Pay-before-forward pacing: every chunk sleeps its own cost
    (len/rate seconds) BEFORE it is forwarded, so even a one-chunk
    response takes its bandwidth share -- a credit/burst bucket would let
    a whole small clone pass at line speed, the opposite of what AM09
    wants to inject. rate = 0 disables."""

    def __init__(self, rate_bytes_per_s: float) -> None:
        self.rate = rate_bytes_per_s

    def take(self, n: int) -> None:
        if self.rate <= 0 or n <= 0:
            return
        time.sleep(n / self.rate)


class ThrottleProxy:
    def __init__(self, args) -> None:
        self.args = args
        self.conn_counter = 0
        self.lock = threading.Lock()

    def next_id(self) -> int:
        with self.lock:
            self.conn_counter += 1
            return self.conn_counter

    def serve(self) -> None:
        srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        srv.bind((self.args.host, self.args.port))
        srv.listen(16)
        srv.settimeout(1.0)
        log(
            f"throttle-git listening on {self.args.host}:{self.args.port} "
            f"mode={self.args.mode} rate={self.args.rate_kb_per_s}KB/s "
            f"latency={self.args.latency_ms}ms stall_after={self.args.stall_after_bytes}",
            self.args.log_file,
        )
        try:
            while True:
                try:
                    client, addr = srv.accept()
                except socket.timeout:
                    continue
                threading.Thread(
                    target=self.handle, args=(client, addr), daemon=True
                ).start()
        except KeyboardInterrupt:
            log("shutting down", self.args.log_file)
        finally:
            srv.close()

    def handle(self, client: socket.socket, addr) -> None:
        conn_id = self.next_id()
        client.settimeout(self.args.timeout)
        remote = None
        try:
            head = read_head(client)
            if not head:
                kill_sock(client)
                return
            reqline = head.split(b"\r\n", 1)[0].decode("latin-1")
            parts = reqline.split()
            if len(parts) < 2:
                kill_sock(client)
                return
            method, url = parts[0].upper(), parts[1]

            if method == "CONNECT":
                host, _, port_s = url.rpartition(":")
                port = int(port_s)
                target = url
                forward_head = None
            else:
                u = urlsplit(url)
                host = u.hostname or ""
                port = u.port or 80
                path = u.path or "/"
                if u.query:
                    path += "?" + u.query
                target = f"{host}:{port}{path}"
                forward_head = to_origin_form(head, path)

            if self.args.mode == "blackhole":
                # Connect succeeds, NO data ever moves: hold the client so a
                # reader without its own timeout hangs forever -- the exact
                # shape bounded-startup assertions are written against.
                log(f"conn#{conn_id} {method} {target} BLACKHOLE (hold "
                    f"{self.args.hold_seconds}s)", self.args.log_file)
                if method == "CONNECT":
                    client.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
                deadline = time.monotonic() + self.args.hold_seconds
                while time.monotonic() < deadline:
                    time.sleep(0.5)
                log(f"conn#{conn_id} blackhole hold over, RST", self.args.log_file)
                kill_sock(client)
                return

            remote = socket.create_connection((host, port), timeout=self.args.timeout)
            if method == "CONNECT":
                client.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
            else:
                remote.sendall(forward_head)
            log(f"conn#{conn_id} {method} {target} forwarded", self.args.log_file)
            self.pump(conn_id, client, remote)
        except (OSError, ValueError) as exc:
            log(f"conn#{conn_id} error: {exc}", self.args.log_file)
        finally:
            if remote is not None:
                try:
                    remote.close()
                except OSError:
                    pass
            graceful_close(client)

    def pump(self, conn_id: int, client: socket.socket, remote: socket.socket) -> None:
        if self.args.latency_ms:
            time.sleep(self.args.latency_ms / 1000.0)
        bucket = LeakyLimiter(self.args.rate_kb_per_s * 1024.0)
        crossed = 0
        t0 = time.monotonic()
        stall_deadline = None
        sockets = [client, remote]
        while True:
            if stall_deadline is not None:
                # Captive-portal stall: hold everything open, move nothing.
                # The client's own timeout must fire first.
                if time.monotonic() > stall_deadline:
                    log(f"conn#{conn_id} stall hold over, RST", self.args.log_file)
                    kill_sock(client)
                    kill_sock(remote)
                    return
                time.sleep(0.5)
                continue
            r, _, _ = select.select(sockets, [], [], 1.0)
            if not r:
                continue
            for src in r:
                dst = remote if src is client else client
                chunk = src.recv(65536)
                if not chunk:
                    elapsed = max(0.001, time.monotonic() - t0)
                    log(
                        f"conn#{conn_id} closed ({crossed} bytes, "
                        f"avg {round(crossed / 1024.0 / elapsed, 1)} KB/s)",
                        self.args.log_file,
                    )
                    return
                bucket.take(len(chunk))
                dst.sendall(chunk)
                crossed += len(chunk)
                if self.args.stall_after_bytes and crossed >= self.args.stall_after_bytes:
                    stall_deadline = time.monotonic() + self.args.hold_seconds
                    log(
                        f"conn#{conn_id} STALL after {crossed} bytes "
                        f"(holding up to {self.args.hold_seconds}s)",
                        self.args.log_file,
                    )
                    break


def main(argv=None) -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--host", default="127.0.0.1")
    p.add_argument("--port", type=int, default=8889)
    p.add_argument("--mode", default="throttle", choices=["throttle", "blackhole", "normal"])
    p.add_argument("--rate-kb-per-s", type=float, default=10.0)
    p.add_argument("--latency-ms", type=int, default=0)
    p.add_argument("--stall-after-bytes", type=int, default=0, help="0 = off")
    p.add_argument(
        "--hold-seconds", type=float, default=120.0,
        help="how long blackhole/stall holds before RST (bounds the harness)",
    )
    p.add_argument("--timeout", type=float, default=60.0)
    p.add_argument("--log-file", default=None)
    args = p.parse_args(argv)
    ThrottleProxy(args).serve()
    return 0


if __name__ == "__main__":
    sys.exit(main())
