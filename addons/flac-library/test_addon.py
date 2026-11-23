"""Tests for the FLAC library addon: `python3 -m unittest -v` in this folder.

`write_flac` makes real FLAC files (uncompressed "verbatim" frames, which every decoder
reads), so no encoder has to be installed.
"""

import http.client
import json
import math
import os
import shutil
import struct
import tempfile
import threading
import unittest
from urllib.parse import quote, urlparse

import aoto_flac_addon as addon

# ---------------------------------------------------------------- a FLAC writer for tests


def _crc(data, poly, bits, top):
    crc = 0
    for byte in data:
        crc ^= byte << (bits - 8)
        for _ in range(8):
            crc = ((crc << 1) ^ poly) & top if crc & (1 << (bits - 1)) else (crc << 1) & top
    return crc


def crc8(data):
    return _crc(data, 0x07, 8, 0xFF)


def crc16(data):
    return _crc(data, 0x8005, 16, 0xFFFF)


def frame_number(n):
    """The frame number in FLAC's UTF-8 style coding."""
    if n < 0x80:
        return bytes([n])
    for count, limit in ((2, 0x800), (3, 0x10000), (4, 0x200000), (5, 0x4000000), (6, 0x80000000)):
        if n < limit:
            lead = (0xFF << (8 - count)) & 0xFF
            out = [lead | (n >> (6 * (count - 1)))]
            for i in range(count - 2, -1, -1):
                out.append(0x80 | ((n >> (6 * i)) & 0x3F))
            return bytes(out)
    raise ValueError(n)


def write_flac(path, seconds=1.0, rate=44100, bits=16, channels=2, tags=None, block=4096):
    """A FLAC file of two sine waves, with Vorbis comment `tags` (a dict)."""
    total = int(seconds * rate)
    amp = (1 << (bits - 1)) - 1
    frames = bytearray()
    done, number = 0, 0
    while done < total:
        n = min(block, total - done)
        header = bytearray([0xFF, 0xF8])
        header.append(((12 if n == block and block == 4096 else 7) << 4) | 0)
        header.append(((channels - 1) << 4) | 0)
        header += frame_number(number)
        if not (n == block and block == 4096):
            header += struct.pack(">H", n - 1)
        header.append(crc8(bytes(header)))
        body = bytearray()
        for ch in range(channels):
            body.append(0x02)  # a VERBATIM subframe
            freq = 440.0 * (1.5 if ch else 1.0)
            for i in range(done, done + n):
                v = int(0.5 * amp * math.sin(2 * math.pi * freq * i / rate))
                body += (v & ((1 << bits) - 1)).to_bytes(bits // 8, "big")
        frame = bytes(header) + bytes(body)
        frames += frame + struct.pack(">H", crc16(frame))
        done += n
        number += 1
    # STREAMINFO: block sizes, frame sizes (unknown, 0), then rate / channels / bits / samples, md5 (unknown, 0)
    info_body = struct.pack(">HH", block, block) + bytes(6)
    info_body += ((rate << 44) | ((channels - 1) << 41) | ((bits - 1) << 36) | total).to_bytes(8, "big") + bytes(16)
    comments = b""
    vendor = b"aoto-test"
    entries = ["%s=%s" % (k.upper(), v) for k, v in (tags or {}).items()]
    comments += struct.pack("<I", len(vendor)) + vendor + struct.pack("<I", len(entries))
    for e in entries:
        raw = e.encode("utf-8")
        comments += struct.pack("<I", len(raw)) + raw
    blocks = bytes([0x00]) + len(info_body).to_bytes(3, "big") + info_body
    # The Vorbis comments are the last metadata block (the top bit says so).
    blocks += bytes([0x80 | 0x04]) + len(comments).to_bytes(3, "big") + comments
    with open(path, "wb") as f:
        f.write(b"fLaC" + blocks + bytes(frames))
    return path


# ---------------------------------------------------------------- tests


class ReadingFiles(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.dir, True)

    def test_a_flac_file_is_read_for_its_tags_and_format(self):
        p = write_flac(os.path.join(self.dir, "x.flac"), seconds=2.0, rate=96000, bits=24,
                       tags={"title": "Hujan di Kemang", "artist": "Senja Kala", "album": "Musim"})
        t = addon.read_flac(p)
        self.assertEqual((t.title, t.artist, t.album), ("Hujan di Kemang", "Senja Kala", "Musim"))
        self.assertEqual((t.sample_rate, t.bit_depth, t.channels), (96000, 24, 2))
        self.assertAlmostEqual(t.duration, 2.0, places=3)
        self.assertEqual(t.size, os.path.getsize(p))
        self.assertGreater(t.bitrate_bps, 1_000_000)

    def test_a_file_without_tags_is_named_by_its_file_name(self):
        p = write_flac(os.path.join(self.dir, "Rona Jingga - Midnight Angkot.flac"))
        t = addon.read_flac(p)
        self.assertEqual((t.artist, t.title), ("Rona Jingga", "Midnight Angkot"))

    def test_a_name_without_a_dash_is_all_title(self):
        t = addon.read_flac(write_flac(os.path.join(self.dir, "Pulang.flac")))
        self.assertEqual((t.artist, t.title), ("", "Pulang"))

    def test_the_album_artist_stands_in_for_a_missing_artist(self):
        t = addon.read_flac(write_flac(os.path.join(self.dir, "a.flac"), tags={"title": "T", "albumartist": "Band"}))
        self.assertEqual(t.artist, "Band")

    def test_something_that_is_not_flac_is_ignored(self):
        p = os.path.join(self.dir, "fake.flac")
        with open(p, "wb") as f:
            f.write(b"ID3 this is an mp3 pretending")
        self.assertIsNone(addon.read_flac(p))
        self.assertIsNone(addon.read_flac(os.path.join(self.dir, "missing.flac")))

    def test_a_cut_off_file_is_ignored_and_does_not_stop_the_scan(self):
        good = write_flac(os.path.join(self.dir, "A - Good.flac"))
        bad = os.path.join(self.dir, "B - Cut.flac")
        with open(good, "rb") as f, open(bad, "wb") as g:
            g.write(f.read(20))
        lib = addon.Library([self.dir])
        self.assertEqual(lib.scan(), 1)

    def test_folders_are_searched_below_and_only_flac_counts(self):
        os.makedirs(os.path.join(self.dir, "deep", "er"))
        write_flac(os.path.join(self.dir, "deep", "er", "A - One.flac"))
        write_flac(os.path.join(self.dir, "B - Two.FLAC"))
        with open(os.path.join(self.dir, "C - Three.mp3"), "wb") as f:
            f.write(b"x")
        self.assertEqual(addon.Library([self.dir]).scan(), 2)


class Searching(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.dir, True)
        for artist, title, album in [("The Weeknd", "Blinding Lights", "After Hours"),
                                     ("Senja Kala", "Hujan di Kemang", "Musim"),
                                     ("Senja Kala", "Lampu Kota", "Musim"),
                                     ("Café Tacvba", "Eres", "Re")]:
            write_flac(os.path.join(self.dir, "%s - %s.flac" % (artist, title)), seconds=0.1,
                       tags={"title": title, "artist": artist, "album": album})
        self.lib = addon.Library([self.dir])
        self.lib.scan()

    def titles(self, q):
        return [t.title for t in self.lib.search(q)]

    def test_title_and_artist_in_the_order_aoto_asks(self):
        self.assertEqual(self.titles("Blinding Lights the weeknd"), ["Blinding Lights"])
        self.assertEqual(self.titles("Blinding Lights"), ["Blinding Lights"])

    def test_every_word_has_to_match(self):
        self.assertEqual(self.titles("hujan kota"), [])
        self.assertEqual(sorted(self.titles("senja kala")), ["Hujan di Kemang", "Lampu Kota"])

    def test_case_and_punctuation_do_not_matter(self):
        self.assertEqual(self.titles("BLINDING, lights!"), ["Blinding Lights"])

    def test_accents_are_kept_and_found(self):
        self.assertEqual(self.titles("café tacvba eres"), ["Eres"])

    def test_the_album_is_searched_too(self):
        self.assertEqual(len(self.titles("musim")), 2)

    def test_an_empty_or_unknown_query_finds_nothing(self):
        self.assertEqual(self.titles(""), [])
        self.assertEqual(self.titles("   "), [])
        self.assertEqual(self.titles("zzzz"), [])

    def test_the_answer_is_limited(self):
        self.assertLessEqual(len(self.lib.search("a", limit=1)), 1)


class Serving(unittest.TestCase):
    TOKEN = ""

    @classmethod
    def setUpClass(cls):
        cls.dir = tempfile.mkdtemp()
        cls.path = write_flac(os.path.join(cls.dir, "Senja Kala - Hujan di Kemang.flac"), seconds=1.0, rate=48000, bits=24,
                              tags={"title": "Hujan di Kemang", "artist": "Senja Kala"})
        with open(cls.path, "rb") as f:
            cls.bytes = f.read()
        cls.server = addon.make_server([cls.dir], token=cls.TOKEN)
        cls.port = cls.server.server_address[1]
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        shutil.rmtree(cls.dir, True)

    def request(self, method, path, headers=None):
        c = http.client.HTTPConnection("127.0.0.1", self.port, timeout=10)
        c.request(method, path, headers=headers or {})
        r = c.getresponse()
        body = r.read()
        c.close()
        return r, body

    def q(self, path, **more):
        sep = "&" if "?" in path else "?"
        extra = dict(more)
        if self.TOKEN:
            extra.setdefault("token", self.TOKEN)
        return path + (sep + "&".join("%s=%s" % (k, quote(v)) for k, v in extra.items()) if extra else "")

    def track_id(self):
        r, body = self.request("GET", self.q("/search", q="hujan kemang"))
        return json.loads(body)["tracks"][0]["id"]

    def test_the_manifest_is_what_aoto_needs(self):
        r, body = self.request("GET", self.q("/manifest.json"))
        m = json.loads(body)
        self.assertEqual(r.status, 200)
        self.assertTrue(m["id"])
        self.assertIn("search", m["resources"])
        quality = [s for s in m["settings"] if s["key"] == "quality"][0]
        self.assertEqual(quality["options"], [{"value": "LOSSLESS"}])

    def test_a_pasted_manifest_address_and_the_bare_address_both_answer(self):
        self.assertEqual(self.request("GET", self.q("/"))[0].status, 200)

    def test_a_search_row_says_lossless_flac_with_the_songs_length(self):
        r, body = self.request("GET", self.q("/search", q="Hujan di Kemang Senja Kala", quality="LOSSLESS"))
        row = json.loads(body)["tracks"][0]
        self.assertEqual((row["title"], row["artist"], row["audioQuality"], row["format"]),
                         ("Hujan di Kemang", "Senja Kala", "LOSSLESS", "flac"))
        self.assertAlmostEqual(row["duration"], 1.0, places=1)

    def test_a_search_with_no_match_is_an_empty_list_not_an_error(self):
        r, body = self.request("GET", self.q("/search", q="nothing like this"))
        self.assertEqual((r.status, json.loads(body)), (200, {"tracks": []}))

    def test_the_stream_answer_describes_the_file(self):
        r, body = self.request("GET", self.q("/stream/" + self.track_id(), quality="LOSSLESS"))
        s = json.loads(body)
        self.assertEqual((s["format"], s["codec"], s["sampleRate"], s["bitDepth"], s["encrypted"]), ("flac", "flac", 48000, 24, False))
        self.assertEqual(s["fileSize"], len(self.bytes))
        # Bits per second, as Aoto reads any number over 3000 (a 24-bit / 48 kHz FLAC is ~2300 kbps).
        self.assertAlmostEqual(s["bitrate"], len(self.bytes) * 8 / 1.0, delta=s["bitrate"] * 0.01)
        self.assertGreater(s["bitrate"], 3000)
        self.assertTrue(s["url"].startswith("http://127.0.0.1:%d/file/" % self.port))
        self.assertTrue(s["url"].split("?")[0].endswith(".flac"))

    def test_the_file_comes_whole_and_unchanged(self):
        s = json.loads(self.request("GET", self.q("/stream/" + self.track_id()))[1])
        u = urlparse(s["url"])
        r, body = self.request("GET", u.path + ("?" + u.query if u.query else ""))
        self.assertEqual((r.status, r.getheader("Content-Type"), r.getheader("Accept-Ranges")), (200, "audio/flac", "bytes"))
        self.assertEqual(body, self.bytes)

    def file_path(self):
        s = json.loads(self.request("GET", self.q("/stream/" + self.track_id()))[1])
        u = urlparse(s["url"])
        return u.path + ("?" + u.query if u.query else "")

    def test_a_range_comes_as_206_with_exactly_those_bytes(self):
        r, body = self.request("GET", self.file_path(), {"Range": "bytes=100-199"})
        self.assertEqual((r.status, body), (206, self.bytes[100:200]))
        self.assertEqual(r.getheader("Content-Range"), "bytes 100-199/%d" % len(self.bytes))
        self.assertEqual(r.getheader("Content-Length"), "100")

    def test_an_open_ended_range_and_a_suffix_range(self):
        n = len(self.bytes)
        r, body = self.request("GET", self.file_path(), {"Range": "bytes=%d-" % (n - 10)})
        self.assertEqual((r.status, body), (206, self.bytes[-10:]))
        r, body = self.request("GET", self.file_path(), {"Range": "bytes=-16"})
        self.assertEqual((r.status, body), (206, self.bytes[-16:]))

    def test_a_range_past_the_end_is_cut_to_the_end(self):
        n = len(self.bytes)
        r, body = self.request("GET", self.file_path(), {"Range": "bytes=%d-%d" % (n - 5, n + 1000)})
        self.assertEqual((r.status, body), (206, self.bytes[-5:]))

    def test_a_range_that_starts_after_the_end_is_refused(self):
        r, _ = self.request("GET", self.file_path(), {"Range": "bytes=%d-" % (len(self.bytes) + 5)})
        self.assertEqual(r.status, 416)
        r, _ = self.request("GET", self.file_path(), {"Range": "bytes=abc"})
        self.assertEqual(r.status, 416)

    def test_head_gives_the_size_without_the_body(self):
        r, body = self.request("HEAD", self.file_path())
        self.assertEqual((r.status, body, r.getheader("Content-Length")), (200, b"", str(len(self.bytes))))

    def test_an_unknown_track_is_not_found(self):
        self.assertEqual(self.request("GET", self.q("/stream/doesnotexist"))[0].status, 404)
        self.assertEqual(self.request("GET", self.q("/file/doesnotexist.flac"))[0].status, 404)

    def test_a_path_cannot_reach_outside_the_library(self):
        for evil in ("/file/../../../../etc/passwd", "/file/%2e%2e%2f%2e%2e%2fetc%2fpasswd", "/file//etc/passwd", "/stream/../../etc/passwd"):
            r, body = self.request("GET", self.q(evil))
            self.assertEqual(r.status, 404, evil)
            self.assertNotIn(b"root:", body)

    def test_other_addresses_are_not_found(self):
        self.assertEqual(self.request("GET", self.q("/admin"))[0].status, 404)


class ServingWithAToken(Serving):
    TOKEN = "a long secret"

    def test_without_the_token_nothing_answers(self):
        for path in ("/manifest.json", "/search?q=hujan", "/stream/x", "/file/x.flac"):
            self.assertEqual(self.request("GET", path)[0].status, 404, path)

    def test_a_wrong_token_is_no_better(self):
        self.assertEqual(self.request("GET", "/manifest.json?token=nope")[0].status, 404)

    def test_the_file_address_carries_the_token_along(self):
        s = json.loads(self.request("GET", self.q("/stream/" + self.track_id()))[1])
        self.assertIn("token=a%20long%20secret", s["url"])


if __name__ == "__main__":
    unittest.main()
