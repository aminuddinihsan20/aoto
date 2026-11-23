#!/usr/bin/env python3
"""A small WebDAV server for testing Aoto's WebDAV source (standard library only).

    python3 tools/webdav_test_server.py ROOT_DIR --port 8801 --user ani --password secret

Serves ROOT_DIR at /music/: PROPFIND (Depth 0 and 1), GET/HEAD with Range, Basic
authentication when a user is given. Not for real use: it has no locking or writes.
"""
import argparse
import base64
import os
import re
import sys
from email.utils import formatdate
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import quote, unquote, urlparse

PREFIX = "/music"


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    root = "."
    user = ""
    password = ""

    def log_message(self, *a):
        pass

    def deny(self):
        self.send_response(HTTPStatus.UNAUTHORIZED)
        self.send_header("WWW-Authenticate", 'Basic realm="aoto test"')
        self.send_header("Content-Length", "0")
        self.end_headers()

    def authorised(self):
        if not self.user:
            return True
        h = self.headers.get("Authorization", "")
        if not h.startswith("Basic "):
            return False
        try:
            u, _, p = base64.b64decode(h[6:]).decode().partition(":")
        except Exception:
            return False
        return u == self.user and p == self.password

    def local(self, url_path):
        p = unquote(urlparse(url_path).path)
        if not (p == PREFIX or p.startswith(PREFIX + "/")):
            return None
        rel = p[len(PREFIX):].lstrip("/")
        full = os.path.normpath(os.path.join(self.root, rel))
        root = os.path.normpath(self.root)
        if full != root and not full.startswith(root + os.sep):
            return None
        return full

    def reply(self, status, body=b"", ctype="text/plain"):
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def do_OPTIONS(self):
        self.send_response(200)
        self.send_header("DAV", "1")
        self.send_header("Allow", "OPTIONS, GET, HEAD, PROPFIND")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_PROPFIND(self):
        # The body is read first: an unread one would be taken for the next request.
        n = int(self.headers.get("Content-Length") or 0)
        if n:
            self.rfile.read(n)
        if not self.authorised():
            return self.deny()
        full = self.local(self.path)
        if full is None or not os.path.exists(full):
            return self.reply(HTTPStatus.NOT_FOUND, b"not found")
        depth = self.headers.get("Depth", "1")
        items = [full]
        if os.path.isdir(full) and depth != "0":
            items += [os.path.join(full, n) for n in sorted(os.listdir(full))]
        out = ['<?xml version="1.0" encoding="utf-8"?><D:multistatus xmlns:D="DAV:">']
        for it in items:
            rel = os.path.relpath(it, self.root).replace(os.sep, "/")
            href = PREFIX + ("" if rel == "." else "/" + rel)
            if os.path.isdir(it):
                href += "/"
                prop = "<D:resourcetype><D:collection/></D:resourcetype>"
            else:
                prop = "<D:resourcetype/><D:getcontentlength>%d</D:getcontentlength>" % os.path.getsize(it)
            out.append("<D:response><D:href>%s</D:href><D:propstat><D:prop>%s</D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>" % (quote(href), prop))
        out.append("</D:multistatus>")
        self.reply(207, "".join(out).encode(), "application/xml; charset=utf-8")

    def do_HEAD(self):
        self.do_GET()

    def do_GET(self):
        if not self.authorised():
            return self.deny()
        full = self.local(self.path)
        if full is None or not os.path.isfile(full):
            return self.reply(HTTPStatus.NOT_FOUND, b"not found")
        size = os.path.getsize(full)
        start, end, status = 0, size - 1, 200
        spec = self.headers.get("Range")
        if spec:
            m = re.fullmatch(r"bytes=(\d*)-(\d*)", spec.strip())
            if m and (m.group(1) or m.group(2)):
                if m.group(1):
                    start = int(m.group(1))
                    if m.group(2):
                        end = min(int(m.group(2)), size - 1)
                else:
                    start = max(0, size - int(m.group(2)))
                if start >= size or start > end:
                    self.send_response(416)
                    self.send_header("Content-Range", "bytes */%d" % size)
                    self.send_header("Content-Length", "0")
                    return self.end_headers()
                status = 206
        self.send_response(status)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Last-Modified", formatdate(os.path.getmtime(full), usegmt=True))
        self.send_header("Content-Length", str(end - start + 1))
        if status == 206:
            self.send_header("Content-Range", "bytes %d-%d/%d" % (start, end, size))
        self.end_headers()
        if self.command == "HEAD":
            return
        try:
            with open(full, "rb") as f:
                f.seek(start)
                left = end - start + 1
                while left > 0:
                    chunk = f.read(min(256 * 1024, left))
                    if not chunk:
                        break
                    self.wfile.write(chunk)
                    left -= len(chunk)
        except (BrokenPipeError, ConnectionResetError):
            pass


def main():
    p = argparse.ArgumentParser()
    p.add_argument("root")
    p.add_argument("--port", type=int, default=8801)
    p.add_argument("--user", default="")
    p.add_argument("--password", default="")
    a = p.parse_args()
    Handler.root, Handler.user, Handler.password = a.root, a.user, a.password
    s = ThreadingHTTPServer(("127.0.0.1", a.port), Handler)
    s.daemon_threads = True
    print("serving %s at http://127.0.0.1:%d%s/" % (a.root, a.port, PREFIX), flush=True)
    try:
        s.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    sys.exit(main())
