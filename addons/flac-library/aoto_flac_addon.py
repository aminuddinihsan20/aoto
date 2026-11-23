#!/usr/bin/env python3
"""A lossless addon for Aoto that serves your own FLAC files.

Point it at the folders where your music is kept; Aoto then plays your FLAC copy of a
song instead of YouTube's whenever you have it. Nothing leaves your network: the server
only answers the phone that asks.

    python3 aoto_flac_addon.py --music ~/Music --token "a long secret"

Then, in Aoto, Settings > Sound & quality > Hi-Res audio > Add a source, paste the link
the server prints. Needs Python 3.9 or newer and nothing else.

The three addresses Aoto uses (the protocol is described in README.md):
    GET /manifest.json            who this addon is
    GET /search?q=title+artist    the songs of yours that match
    GET /stream/<id>              where the file is, and what it is
    GET /file/<id>.flac           the file itself (with Range requests)
"""

import argparse
import hashlib
import hmac
import json
import os
import re
import socket
import struct
import sys
import threading
import time
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, quote, urlparse

NAME = "Aoto FLAC library"
VERSION = "1.0.0"
TIER = "LOSSLESS"
SEARCH_LIMIT = 25
RESCAN_SECONDS = 60


# ---------------------------------------------------------------- reading FLAC files


class Track:
    """One FLAC file: what Aoto needs to know of it."""

    def __init__(self, path, title, artist, album, sample_rate, bit_depth, channels, samples, size):
        self.path = path
        self.title = title
        self.artist = artist
        self.album = album
        self.sample_rate = sample_rate
        self.bit_depth = bit_depth
        self.channels = channels
        self.samples = samples
        self.size = size
        self.id = hashlib.sha1(path.encode("utf-8", "surrogateescape")).hexdigest()[:16]

    @property
    def duration(self):
        return self.samples / self.sample_rate if self.sample_rate else 0.0

    @property
    def bitrate_bps(self):
        return int(self.size * 8 / self.duration) if self.duration else 0

    def haystack(self):
        return normalize(" ".join((self.artist, self.title, self.album)))


def normalize(text):
    """Lower case, accents kept, everything that is not a letter or digit a space."""
    return " ".join(re.findall(r"\w+", text.lower()))


def read_flac(path):
    """The Track a FLAC file describes, or None when it is not a readable FLAC file."""
    try:
        size = os.path.getsize(path)
        with open(path, "rb") as f:
            if f.read(4) != b"fLaC":
                return None
            info = None
            tags = {}
            while True:
                header = f.read(4)
                if len(header) < 4:
                    break
                last = header[0] & 0x80
                kind = header[0] & 0x7F
                length = int.from_bytes(header[1:4], "big")
                body = f.read(length)
                if len(body) < length:
                    return None
                if kind == 0 and length >= 34:
                    packed = int.from_bytes(body[10:18], "big")
                    info = (
                        (packed >> 44) & 0xFFFFF,  # sample rate
                        ((packed >> 41) & 0x7) + 1,  # channels
                        ((packed >> 36) & 0x1F) + 1,  # bits per sample
                        packed & 0xFFFFFFFFF,  # total samples
                    )
                elif kind == 4:
                    tags = parse_vorbis_comments(body)
                if last:
                    break
        if info is None or info[0] == 0:
            return None
    except OSError:
        return None
    rate, channels, bits, samples = info
    stem = os.path.splitext(os.path.basename(path))[0]
    title = tags.get("title") or title_from_name(stem)
    artist = tags.get("artist") or tags.get("albumartist") or artist_from_name(stem)
    return Track(path, title, artist, tags.get("album", ""), rate, bits, channels, samples, size)


def parse_vorbis_comments(body):
    tags = {}
    try:
        vendor = struct.unpack_from("<I", body, 0)[0]
        at = 4 + vendor
        count = struct.unpack_from("<I", body, at)[0]
        at += 4
        for _ in range(count):
            n = struct.unpack_from("<I", body, at)[0]
            at += 4
            entry = body[at:at + n].decode("utf-8", "replace")
            at += n
            if "=" in entry:
                key, value = entry.split("=", 1)
                tags.setdefault(key.lower(), value.strip())
    except struct.error:
        pass
    return tags


def title_from_name(stem):
    """'Artist - Title' names carry both; anything else is all title."""
    return stem.split(" - ", 1)[1].strip() if " - " in stem else stem


def artist_from_name(stem):
    return stem.split(" - ", 1)[0].strip() if " - " in stem else ""


class Library:
    """The tracks under the music folders, rescanned when a minute has passed."""

    def __init__(self, folders):
        self.folders = [os.path.abspath(os.path.expanduser(f)) for f in folders]
        self.lock = threading.Lock()
        self.tracks = {}
        self.scanned = 0.0

    def scan(self):
        found = {}
        for folder in self.folders:
            for root, _dirs, files in os.walk(folder):
                for name in files:
                    if name.lower().endswith(".flac"):
                        track = read_flac(os.path.join(root, name))
                        if track is not None:
                            found[track.id] = track
        with self.lock:
            self.tracks = found
            self.scanned = time.monotonic()
        return len(found)

    def all(self):
        if time.monotonic() - self.scanned > RESCAN_SECONDS:
            self.scan()
        with self.lock:
            return list(self.tracks.values())

    def get(self, track_id):
        self.all()
        with self.lock:
            return self.tracks.get(track_id)

    def search(self, query, limit=SEARCH_LIMIT):
        """Tracks whose artist, title and album hold every word of `query`, closest first."""
        words = normalize(query).split()
        if not words:
            return []
        scored = []
        for track in self.all():
            hay = track.haystack()
            parts = set(hay.split())
            if all(w in parts or w in hay for w in words):
                exact = sum(1 for w in words if w in parts)
                scored.append((-exact, len(hay), track))
        scored.sort(key=lambda s: s[:2])
        return [t for _, _, t in scored[:limit]]


# ---------------------------------------------------------------- the server


class Handler(BaseHTTPRequestHandler):
    server_version = "AotoFlacAddon/" + VERSION
    protocol_version = "HTTP/1.1"

    library = None  # set by serve()
    token = ""

    # -- plumbing
    def log_message(self, fmt, *args):
        if self.server.verbose:
            sys.stderr.write("%s %s\n" % (self.address_string(), fmt % args))

    def send_json(self, payload, status=HTTPStatus.OK):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def authorised(self, query):
        if not self.token:
            return True
        given = (query.get("token") or [""])[0]
        return hmac.compare_digest(given.encode(), self.token.encode())

    def base_url(self):
        host = self.headers.get("Host") or "%s:%d" % self.server.server_address[:2]
        return "http://" + host

    def token_query(self):
        return "?token=" + quote(self.token, safe="") if self.token else ""

    # -- routing
    def do_HEAD(self):
        self.do_GET()

    def do_GET(self):
        url = urlparse(self.path)
        query = parse_qs(url.query)
        if not self.authorised(query):
            return self.send_json({"error": "not found"}, HTTPStatus.NOT_FOUND)
        path = url.path.rstrip("/") or "/"
        if path in ("/", "/manifest.json"):
            return self.manifest()
        if path == "/search":
            return self.search((query.get("q") or [""])[0])
        if path.startswith("/stream/"):
            return self.stream(path[len("/stream/"):])
        if path.startswith("/file/"):
            return self.file(path[len("/file/"):])
        return self.send_json({"error": "not found"}, HTTPStatus.NOT_FOUND)

    def manifest(self):
        self.send_json({
            "id": "aoto.flac.library",
            "name": NAME,
            "version": VERSION,
            "description": "Your own FLAC files, played by Aoto in place of YouTube's copy.",
            "resources": ["search", "stream"],
            "types": ["track"],
            "settings": [{"key": "quality", "default": TIER, "options": [{"value": TIER}]}],
        })

    def search(self, q):
        rows = []
        for t in self.library.search(q):
            rows.append({
                "id": t.id,
                "title": t.title,
                "artist": t.artist,
                "album": t.album,
                "duration": round(t.duration, 1),
                "audioQuality": TIER,
                "format": "flac",
                "provider": "Local library",
            })
        self.send_json({"tracks": rows})

    def stream(self, track_id):
        t = self.library.get(track_id)
        if t is None:
            return self.send_json({"error": "track not found"}, HTTPStatus.NOT_FOUND)
        self.send_json({
            "url": "%s/file/%s.flac%s" % (self.base_url(), t.id, self.token_query()),
            "format": "flac",
            "codec": "flac",
            "container": "flac",
            "mimeType": "audio/flac",
            "quality": TIER,
            "sampleRate": t.sample_rate,
            "bitDepth": t.bit_depth,
            # In bits per second: Aoto reads a number over 3000 that way, and a hi-res
            # FLAC is over 3000 kbps (96 kHz / 24-bit is about 4600).
            "bitrate": t.bitrate_bps,
            "fileSize": t.size,
            "duration": round(t.duration, 1),
            "encrypted": False,
        })

    def file(self, name):
        track_id = name[:-5] if name.endswith(".flac") else name
        t = self.library.get(track_id)
        if t is None:
            return self.send_json({"error": "track not found"}, HTTPStatus.NOT_FOUND)
        try:
            size = os.path.getsize(t.path)
        except OSError:
            return self.send_json({"error": "file is gone"}, HTTPStatus.NOT_FOUND)
        start, end = 0, size - 1
        status = HTTPStatus.OK
        spec = self.headers.get("Range")
        if spec:
            m = re.fullmatch(r"bytes=(\d*)-(\d*)", spec.strip())
            if not m or (not m.group(1) and not m.group(2)):
                return self.range_not_satisfiable(size)
            if m.group(1):
                start = int(m.group(1))
                if m.group(2):
                    end = min(int(m.group(2)), size - 1)
            else:  # the last N bytes
                start = max(0, size - int(m.group(2)))
            if start >= size or start > end:
                return self.range_not_satisfiable(size)
            status = HTTPStatus.PARTIAL_CONTENT
        length = end - start + 1
        self.send_response(status)
        self.send_header("Content-Type", "audio/flac")
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(length))
        if status == HTTPStatus.PARTIAL_CONTENT:
            self.send_header("Content-Range", "bytes %d-%d/%d" % (start, end, size))
        self.end_headers()
        if self.command == "HEAD":
            return
        try:
            with open(t.path, "rb") as f:
                f.seek(start)
                left = length
                while left > 0:
                    chunk = f.read(min(256 * 1024, left))
                    if not chunk:
                        break
                    self.wfile.write(chunk)
                    left -= len(chunk)
        except (BrokenPipeError, ConnectionResetError):
            pass  # the phone stopped listening (a seek, or the song changed)

    def range_not_satisfiable(self, size):
        self.send_response(HTTPStatus.REQUESTED_RANGE_NOT_SATISFIABLE)
        self.send_header("Content-Range", "bytes */%d" % size)
        self.send_header("Content-Length", "0")
        self.end_headers()


class Server(ThreadingHTTPServer):
    daemon_threads = True
    verbose = False


def make_server(folders, host="127.0.0.1", port=0, token="", verbose=False):
    """A server over `folders`, already scanned, not yet serving."""
    handler = type("BoundHandler", (Handler,), {"library": Library(folders), "token": token})
    handler.library.scan()
    server = Server((host, port), handler)
    server.verbose = verbose
    return server


def lan_addresses():
    """The addresses this computer has on the local network, for the link to print."""
    found = []
    try:
        for info in socket.getaddrinfo(socket.gethostname(), None, socket.AF_INET):
            ip = info[4][0]
            if not ip.startswith("127.") and ip not in found:
                found.append(ip)
    except OSError:
        pass
    try:  # the address used to reach the outside is the one the phone can reach too
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.connect(("10.255.255.255", 1))
        ip = s.getsockname()[0]
        s.close()
        if ip not in found and not ip.startswith("127."):
            found.insert(0, ip)
    except OSError:
        pass
    return found


def main(argv=None):
    p = argparse.ArgumentParser(description="Serve your own FLAC files to Aoto.")
    p.add_argument("--music", action="append", required=True, metavar="DIR", help="a folder with FLAC files (repeat for more)")
    p.add_argument("--host", default="0.0.0.0", help="address to listen on (default: every one, so the phone can reach it)")
    p.add_argument("--port", type=int, default=8787)
    p.add_argument("--token", default="", help="a secret the link must carry; recommended on any network you share")
    p.add_argument("--verbose", action="store_true", help="log every request")
    args = p.parse_args(argv)

    for folder in args.music:
        if not os.path.isdir(os.path.expanduser(folder)):
            p.error("not a folder: %s" % folder)
    server = make_server(args.music, args.host, args.port, args.token, args.verbose)
    count = len(server.RequestHandlerClass.library.tracks)
    print("%s %s: %d FLAC file(s) found." % (NAME, VERSION, count))
    if count == 0:
        print("Nothing to serve yet: no .flac files under " + ", ".join(args.music))
    suffix = "?token=" + quote(args.token, safe="") if args.token else ""
    hosts = lan_addresses() if args.host in ("0.0.0.0", "") else [args.host]
    print("Paste one of these in Aoto (Settings > Sound & quality > Hi-Res audio > Add a source):")
    for ip in hosts or ["127.0.0.1"]:
        print("    http://%s:%d%s" % (ip, server.server_address[1], suffix))
    if not args.token and args.host in ("0.0.0.0", ""):
        print("Note: without --token, anyone on this network can list and download these files.")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nStopped.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
