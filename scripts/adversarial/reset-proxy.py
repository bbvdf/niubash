#!/usr/bin/env python3
"""reset-proxy.py -- a local HTTP proxy that kills traffic on purpose.

Simulates the hostile-network axis (adversarial-matrix lane AM01): the
"five TLS-burned release runs" class (2026-10-03, docs/journey-gate.md
"Network reality") -- TLS/TCP resets mid-clone and mid-fetch. niu's git
traffic goes through this proxy via `-c http.proxy=http://127.0.0.1:PORT`
(or https_proxy/HTTPS_PROXY in the sandbox env), so the failure shape is
EXACTLY what a user on a hostile network sees: curl 56 / "Recv failure /
connection was reset" / "unexpected eof while reading" -- at a
deterministic moment, offline.

Forwarding styles (both handled):
  CONNECT host:port   https tunnels (the git->github.com shape; libcurl
                      tunnels with CONNECT, then speaks TLS inside)
  absolute-form GET   plain http proxies WITHOUT CONNECT: libcurl sends
                      `GET http://host/path HTTP/1.1` straight to the
                      proxy -- so local http fixtures work too

Modes:
  normal              clean forwarding (baseline control -- proves the
                      proxy core itself is not the failure)
  reset-handshake     accept, then RST immediately (reset at handshake)
  reset-after-bytes   forward normally, RST after N bytes have crossed
                      (reset MID-CLONE / mid-pack download)
  reset-after-seconds forward normally, RST T seconds after open
  reset-every         reset at handshake every Kth connection, others
                      clean (reset-storm shape: some clones live, some die)

Deterministic, offline, no admin, stdlib only.

Usage (lane probe example -- reset mid-clone):
    python scripts/adversarial/reset-proxy.py --port 8888 \
        --mode reset-after-bytes --bytes 4096 --log-file proxy.log &
    git -c http.proxy=http://127.0.0.1:8888 clone https://github.com/x/y.git
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


def rst_close(sock: socket.socket) -> None:
    """Close with SO_LINGER(0) so the peer sees a TCP RST, not a FIN.

    This is the whole point of the tool: libcurl reports a RST mid-read as
    "Recv failure / connection was reset / unexpected eof" -- the exact
    schannel/curl 56 shape the release runs burned on.
    """
    try:
        sock.setsockopt(
            socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0)
        )
    except OSError:
        pass
    try:
        sock.shutdown(socket.SHUT_RDWR)
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


class ProxyServer:
    def __init__(self, args) -> None:
        self.args = args
        self.conn_counter = 0
        self.counter_lock = threading.Lock()

    def next_conn_id(self) -> int:
        with self.counter_lock:
            self.conn_counter += 1
            return self.conn_counter

    def serve(self) -> None:
        srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        srv.bind((self.args.host, self.args.port))
        srv.listen(16)
        srv.settimeout(1.0)
        extra = ""
        if self.args.mode == "reset-after-bytes":
            extra = f" bytes={self.args.bytes}"
        elif self.args.mode == "reset-after-seconds":
            extra = f" seconds={self.args.seconds}"
        elif self.args.mode == "reset-every":
            extra = f" every={self.args.every}"
        log(
            f"reset-proxy listening on {self.args.host}:{self.args.port} "
            f"mode={self.args.mode}{extra}",
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
        conn_id = self.next_conn_id()
        client.settimeout(self.args.timeout)
        log(f"conn#{conn_id} accepted from {addr[0]}:{addr[1]}", self.args.log_file)
        remote = None
        try:
            head = read_head(client)
            if not head:
                rst_close(client)
                return
            reqline = head.split(b"\r\n", 1)[0].decode("latin-1")
            parts = reqline.split()
            if len(parts) < 2:
                rst_close(client)
                return
            method, url = parts[0].upper(), parts[1]

            if method == "CONNECT":
                host, _, port = url.rpartition(":")
                port = int(port)
                target = url
                open_reset = self.should_reset(conn_id)
            else:
                u = urlsplit(url)
                host = u.hostname or ""
                port = u.port or 80
                path = u.path or "/"
                if u.query:
                    path += "?" + u.query
                target = f"{host}:{port}{path}"
                open_reset = self.should_reset(conn_id)

            if self.args.allow_host and host not in self.args.allow_host:
                log(f"conn#{conn_id} REFUSED (not in --allow-host): {host}", self.args.log_file)
                rst_close(client)
                return

            if open_reset:
                # For CONNECT: reply 200 so the client commits to the
                # tunnel, then RST -- the TLS-handshake-reset shape. For
                # plain http: connect upstream, forward the request, then
                # RST before any response bytes cross (server-side reset).
                log(f"conn#{conn_id} {method} {target} -> RST", self.args.log_file)
                if method == "CONNECT":
                    client.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
                    time.sleep(0.05)
                    rst_close(client)
                else:
                    try:
                        remote = socket.create_connection((host, port), timeout=self.args.timeout)
                        remote.sendall(to_origin_form(head, path))
                        time.sleep(0.05)
                        rst_close(remote)
                    except OSError as exc:
                        log(f"conn#{conn_id} upstream failed: {exc}", self.args.log_file)
                    rst_close(client)
                return

            remote = socket.create_connection((host, port), timeout=self.args.timeout)
            if method == "CONNECT":
                client.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
            else:
                remote.sendall(to_origin_form(head, path))
            log(f"conn#{conn_id} {method} {target} forwarded", self.args.log_file)
            killed = self.pump(conn_id, client, remote)
            if killed:
                rst_close(client)
        except (OSError, ValueError) as exc:
            log(f"conn#{conn_id} error: {exc}", self.args.log_file)
        finally:
            if remote is not None:
                try:
                    remote.close()
                except OSError:
                    pass
            graceful_close(client)

    def pump(self, conn_id: int, client: socket.socket, remote: socket.socket) -> bool:
        """Returns True when this proxy killed the transfer on purpose."""
        opened = time.monotonic()
        crossed = 0
        sockets = [client, remote]
        while True:
            if self.args.mode == "reset-after-seconds" and time.monotonic() - opened >= self.args.seconds:
                log(f"conn#{conn_id} RST after {self.args.seconds}s ({crossed} bytes)", self.args.log_file)
                rst_close(client)
                rst_close(remote)
                return True
            r, _, _ = select.select(sockets, [], [], 1.0)
            if not r:
                continue
            for src in r:
                dst = remote if src is client else client
                chunk = src.recv(65536)
                if not chunk:
                    log(f"conn#{conn_id} closed normally ({crossed} bytes crossed)", self.args.log_file)
                    return False
                # Decide the reset BEFORE forwarding: RSTing after a
                # completed small response would still let the client
                # succeed (it just reconnects). Dropping the crossing
                # chunk kills the transfer mid-body.
                if self.args.mode == "reset-after-bytes" and crossed + len(chunk) >= self.args.bytes:
                    log(
                        f"conn#{conn_id} RST mid-transfer at {crossed} bytes, "
                        f"dropping {len(chunk)}-byte chunk (threshold {self.args.bytes})",
                        self.args.log_file,
                    )
                    rst_close(client)
                    rst_close(remote)
                    return True
                dst.sendall(chunk)
                crossed += len(chunk)

    def should_reset(self, conn_id: int) -> bool:
        mode = self.args.mode
        if mode == "reset-handshake":
            return True
        if mode == "reset-every":
            return conn_id % max(1, self.args.every) == 0
        return False


def main(argv=None) -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--host", default="127.0.0.1")
    p.add_argument("--port", type=int, default=8888)
    p.add_argument(
        "--mode",
        default="normal",
        choices=[
            "normal",
            "reset-handshake",
            "reset-after-bytes",
            "reset-after-seconds",
            "reset-every",
        ],
    )
    p.add_argument("--bytes", type=int, default=4096, help="threshold for reset-after-bytes")
    p.add_argument("--seconds", type=float, default=2.0, help="delay for reset-after-seconds")
    p.add_argument("--every", type=int, default=2, help="K for reset-every")
    p.add_argument("--timeout", type=float, default=60.0, help="socket idle timeout")
    p.add_argument("--allow-host", action="append", default=[], help="repeatable; empty = any")
    p.add_argument("--log-file", default=None)
    args = p.parse_args(argv)

    if args.mode == "reset-after-bytes" and args.bytes <= 0:
        p.error("--bytes must be > 0 for reset-after-bytes")
    ProxyServer(args).serve()
    return 0


if __name__ == "__main__":
    sys.exit(main())
