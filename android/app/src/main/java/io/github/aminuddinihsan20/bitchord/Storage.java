package io.github.aminuddinihsan20.bitchord;

import android.content.ContentResolver;
import android.content.Context;
import android.database.Cursor;
import android.media.MediaMetadataRetriever;
import android.net.Uri;
import android.provider.DocumentsContract;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Locale;

/**
 * Work on folders the listener chose (Android's Storage Access Framework): list the music
 * in one, copy a file out of one to somewhere the app can read, copy a download into
 * one. Each runs on its own thread and reports back to the app by call id.
 */
final class Storage {
    private static final char FIELD = '\u001f';
    private static final List<String> AUDIO_EXTENSIONS = Arrays.asList("mp3", "m4a", "aac", "flac", "wav", "ogg", "oga", "opus", "alac", "mka", "wma");
    private static final long CACHE_LIMIT = 400L * 1024 * 1024;

    /** The result of a job: ok with text, or not ok with why. */
    interface Done {
        void finish(boolean ok, String text);
    }

    private Storage() {
    }

    /** Every music file under `tree`, one per line: uri, title, artist, album, duration (ms), size. */
    static void scan(final Context context, final String treeUri, final Done done) {
        new Thread(() -> {
            try {
                Uri tree = Uri.parse(treeUri);
                StringBuilder out = new StringBuilder();
                walk(context, tree, DocumentsContract.getTreeDocumentId(tree), 0, out);
                done.finish(true, out.toString());
            } catch (Exception e) {
                done.finish(false, String.valueOf(e.getMessage()));
            }
        }, "storage-scan").start();
    }

    private static void walk(Context context, Uri tree, String documentId, int depth, StringBuilder out) {
        if (depth > 10) {
            return;
        }
        ContentResolver resolver = context.getContentResolver();
        Uri children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, documentId);
        String[] columns = {DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME, DocumentsContract.Document.COLUMN_MIME_TYPE, DocumentsContract.Document.COLUMN_SIZE};
        List<String[]> found = new ArrayList<>();
        try (Cursor c = resolver.query(children, columns, null, null, null)) {
            while (c != null && c.moveToNext()) {
                found.add(new String[] {c.getString(0), c.getString(1), c.getString(2), String.valueOf(c.isNull(3) ? 0 : c.getLong(3))});
            }
        } catch (Exception e) {
            return;
        }
        for (String[] f : found) {
            if (DocumentsContract.Document.MIME_TYPE_DIR.equals(f[2])) {
                walk(context, tree, f[0], depth + 1, out);
            } else if (isAudio(f[1], f[2])) {
                Uri doc = DocumentsContract.buildDocumentUriUsingTree(tree, f[0]);
                out.append(describe(context, doc, f[1], f[3])).append('\n');
            }
        }
    }

    private static boolean isAudio(String name, String mime) {
        if (mime != null && mime.startsWith("audio/")) {
            return true;
        }
        return AUDIO_EXTENSIONS.contains(extension(name));
    }

    private static String extension(String name) {
        int dot = name == null ? -1 : name.lastIndexOf('.');
        return dot < 0 ? "" : name.substring(dot + 1).toLowerCase(Locale.ROOT);
    }

    private static String clean(String s) {
        return s == null ? "" : s.replace(FIELD, ' ').replace('\n', ' ').replace('\r', ' ').trim();
    }

    private static String describe(Context context, Uri doc, String fileName, String size) {
        String title = "", artist = "", album = "";
        long duration = 0;
        MediaMetadataRetriever r = new MediaMetadataRetriever();
        try {
            r.setDataSource(context, doc);
            title = clean(r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_TITLE));
            artist = clean(r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_ARTIST));
            if (artist.isEmpty()) {
                artist = clean(r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_ALBUMARTIST));
            }
            album = clean(r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_ALBUM));
            String d = r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION);
            duration = d == null ? 0 : Long.parseLong(d);
        } catch (Exception ignored) {
            // No tags: the file name stands in for the title.
        } finally {
            try {
                r.release();
            } catch (Exception ignored) {
                // Nothing to release.
            }
        }
        if (title.isEmpty()) {
            int dot = fileName.lastIndexOf('.');
            title = clean(dot > 0 ? fileName.substring(0, dot) : fileName);
        }
        return doc + String.valueOf(FIELD) + title + FIELD + artist + FIELD + album + FIELD + duration + FIELD + size;
    }

    /** Copy a document into the app's cache, where the app can read it as a file; answers its path. */
    static void fetch(final Context context, final String docUri, final Done done) {
        new Thread(() -> {
            try {
                Uri doc = Uri.parse(docUri);
                File dir = new File(context.getCacheDir(), "local");
                if (!dir.isDirectory() && !dir.mkdirs()) {
                    throw new IllegalStateException("no cache folder");
                }
                File file = new File(dir, Integer.toHexString(docUri.hashCode()) + "." + (extension(docUri).isEmpty() ? "audio" : extension(docUri)));
                if (!file.isFile() || file.length() == 0) {
                    trim(dir);
                    File part = new File(dir, file.getName() + ".part");
                    try (InputStream in = context.getContentResolver().openInputStream(doc); OutputStream out = new FileOutputStream(part)) {
                        if (in == null) {
                            throw new IllegalStateException("can't open the file");
                        }
                        copy(in, out);
                    }
                    if (!part.renameTo(file)) {
                        throw new IllegalStateException("can't keep the copy");
                    }
                }
                done.finish(true, file.getAbsolutePath());
            } catch (Exception e) {
                done.finish(false, String.valueOf(e.getMessage()));
            }
        }, "storage-fetch").start();
    }

    /** Keep the cache under its limit by dropping the oldest copies. */
    private static void trim(File dir) {
        File[] files = dir.listFiles();
        if (files == null) {
            return;
        }
        Arrays.sort(files, (a, b) -> Long.compare(a.lastModified(), b.lastModified()));
        long total = 0;
        for (File f : files) {
            total += f.length();
        }
        for (File f : files) {
            if (total <= CACHE_LIMIT) {
                break;
            }
            total -= f.length();
            // Only deleting; a copy being played from is open and survives on Android.
            f.delete();
        }
    }

    /** Copy `path` into the folder `treeUri` as `name`; answers where it went. */
    static void export(final Context context, final String treeUri, final String path, final String name, final String mime, final Done done) {
        new Thread(() -> {
            try {
                Uri tree = Uri.parse(treeUri);
                Uri parent = DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree));
                ContentResolver resolver = context.getContentResolver();
                Uri doc = DocumentsContract.createDocument(resolver, parent, mime, name);
                if (doc == null) {
                    throw new IllegalStateException("can't create the file");
                }
                try (InputStream in = new FileInputStream(path); OutputStream out = resolver.openOutputStream(doc)) {
                    if (out == null) {
                        throw new IllegalStateException("can't write the file");
                    }
                    copy(in, out);
                }
                done.finish(true, name);
            } catch (Exception e) {
                done.finish(false, String.valueOf(e.getMessage()));
            }
        }, "storage-export").start();
    }

    static void copy(InputStream in, OutputStream out) throws java.io.IOException {
        byte[] buffer = new byte[64 * 1024];
        int n;
        while ((n = in.read(buffer)) > 0) {
            out.write(buffer, 0, n);
        }
    }
}
