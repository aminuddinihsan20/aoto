# Aoto FLAC library: a lossless addon for your own music

A small server that serves the **FLAC files you already have** to Aoto. When Aoto plays a
song from YouTube Music that you also own as FLAC, it plays your file instead, and the
player shows *Lossless* or *Hi-Res Lossless*.

It does not find, fetch or convert music. If you have no FLAC of a song, YouTube plays as
before. Python 3.9 or newer, nothing to install.

## Use it

On the computer that holds your music:

```sh
python3 aoto_flac_addon.py --music ~/Music --token "a-long-secret"
```

It scans the folder (and the folders below it), then prints links like:

```
Aoto FLAC library 1.0.0: 1204 FLAC file(s) found.
Paste one of these in Aoto (Settings > Sound & quality > Hi-Res audio > Add a source):
    http://192.168.1.20:8787?token=a-long-secret
```

On the phone, on the **same Wi-Fi**: Aoto › Settings › Sound & quality › *Hi-Res audio* ›
*Add a source*, and paste the link. Aoto tests it and says what it found, for example
"Aoto FLAC library · Local library · plays Lossless". Leave the server running while you
listen. `--music` can be repeated for more folders; `--port` changes the port.

- Away from home, use a VPN such as Tailscale and paste the computer's address there.
- A firewall may ask whether to let Python receive connections: allow it on the private network.
- Without `--token`, anyone on the network can list and download the files. Use a token.
- A folder is rescanned every minute, so new files appear without restarting.

## How a song is matched

Aoto asks for the song's title and first artist, then plays your file only if all of this
holds (Aoto's rules, not the server's):

- the title's words are the same (a "Remastered" or "Live" mark must match too);
- the artist's name is in the file's artist;
- **the file's length is within 3 seconds of the song's on YouTube Music**, so a remix,
  an edit or a different master does not stand in for the song.

So tag your files (title, artist), or name them `Artist - Title.flac`; the file name is
used when a file has no tags. If a song you own does not play from your file, the length
is the usual reason: the version you own is not the one YouTube has.

## What it does not do

- Only `.flac` files. (ALAC, WAV and others are not served.)
- Downloads in Aoto still save YouTube's audio, not your FLAC.
- On mobile data the file is several times larger than YouTube's stream. Aoto's *Lossless
  from addons* setting can keep it to Wi-Fi.

## For people who want to write their own addon

An addon is a web server that answers three GET addresses with JSON. This one is about
400 lines; read `aoto_flac_addon.py` for a working example.

`GET /manifest.json`

```json
{ "id": "aoto.flac.library", "name": "Aoto FLAC library", "version": "1.0.0",
  "resources": ["search", "stream"],
  "settings": [{ "key": "quality", "default": "LOSSLESS", "options": [{ "value": "LOSSLESS" }] }] }
```

`id` is required. `resources`, when given, must include `"search"`. Aoto sends the
`quality` option (and the default of any other setting) on every call.

`GET /search?q=<title artist>&quality=LOSSLESS`

```json
{ "tracks": [{ "id": "c03671ea9baf9413", "title": "Blinding Lights", "artist": "The Weeknd",
               "duration": 200.4, "audioQuality": "LOSSLESS", "format": "flac" }] }
```

`id` and `title` are required. `duration` is in seconds. Aoto does the matching itself, so
return everything plausible rather than guessing.

`GET /stream/<id>?quality=LOSSLESS`

```json
{ "url": "http://192.168.1.20:8787/file/c03671ea9baf9413.flac",
  "format": "flac", "codec": "flac", "quality": "LOSSLESS",
  "sampleRate": 44100, "bitDepth": 16, "bitrate": 1411200,
  "fileSize": 35200000, "encrypted": false }
```

- `url` must be http(s) and **must answer Range requests** (`206`), which Aoto uses to
  download in parts and to seek; without them it falls back to one slow download.
- `bitrate`: a number over 3000 is read as bits per second, anything lower as kbps.
- `sampleRate` under 1000 is read as kHz. `fileSize` avoids an extra request for it.
- A stream is used only if it is lossless (FLAC, ALAC, WAV) or Dolby Atmos; HLS, encrypted
  and E-AC-3 streams are skipped.
- Time limits: search and manifest 8 seconds, stream 18.

## Tests

```sh
python3 -m unittest -v          # 45 tests: reading files, search, serving, Range, tokens
```

`test_addon.py` writes real FLAC files itself, so nothing else is needed. The Aoto side
(its own client, ranged download, decoder and seek) is checked against a running server
with `YTM_FLAC_ADDON=<link> YTM_FLAC_TRACK="title|artist|seconds|rate|channels" cargo test
--lib live_addon_plays -- --ignored --nocapture`.
