package io.github.aminuddinihsan20.aoto;

import android.app.Activity;
import android.content.ClipData;
import android.content.ContentResolver;
import android.content.ContentValues;
import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.BitmapShader;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.LinearGradient;
import android.graphics.Matrix;
import android.graphics.Paint;
import android.graphics.RectF;
import android.graphics.Shader;
import android.graphics.Typeface;
import android.graphics.drawable.Drawable;
import android.media.MediaScannerConnection;
import android.net.Uri;
import android.os.Build;
import android.os.Environment;
import android.provider.MediaStore;
import android.text.Layout;
import android.text.StaticLayout;
import android.text.TextDirectionHeuristics;
import android.text.TextPaint;
import android.text.TextUtils;

import androidx.core.content.ContextCompat;
import androidx.core.content.FileProvider;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.List;

/**
 * Lyrics as a picture: draws the card the app describes (src/lyric_card.rs: the lines, the
 * song, the colours, the size) into a PNG in the app's cache, and saves that picture to the
 * phone's Pictures or hands it to the share sheet. Android lays the text out, so every
 * script is shaped and wrapped as the phone does it everywhere else.
 */
final class LyricCard {
    /** The description format this draws (`VERSION` in src/lyric_card.rs). */
    private static final int VERSION = 1;
    /** Under the cache folder; the only folder the share provider serves (res/xml/share_paths.xml). */
    private static final String FOLDER = "share";
    /** Pictures older than this are cleared away when the next one is drawn. */
    private static final long KEEP_MS = 60 * 60 * 1000L;
    /** The lines are never set smaller than this, in pixels. */
    private static final float SMALLEST = 14f;

    private LyricCard() {
    }

    /** Draw the card `json` describes on a thread of its own; answers the PNG's path. */
    static void render(final Context context, final String json, final Storage.Done done) {
        new Thread(() -> {
            try {
                JSONObject card = new JSONObject(json);
                if (card.getInt("v") != VERSION) {
                    throw new IllegalStateException("a card this version can't draw");
                }
                Bitmap bitmap = draw(context, card);
                File folder = folder(context);
                sweep(folder);
                // Named by what it shows: the same card drawn again replaces itself.
                File file = new File(folder, "lyrics-" + Integer.toHexString(json.hashCode()) + ".png");
                File part = new File(folder, file.getName() + ".part");
                try (OutputStream out = new FileOutputStream(part)) {
                    if (!bitmap.compress(Bitmap.CompressFormat.PNG, 100, out)) {
                        throw new IllegalStateException("can't write the picture");
                    }
                } finally {
                    bitmap.recycle();
                }
                if (!part.renameTo(file)) {
                    throw new IllegalStateException("can't keep the picture");
                }
                done.finish(true, file.getAbsolutePath());
            } catch (Exception | OutOfMemoryError e) {
                done.finish(false, String.valueOf(e.getMessage()));
            }
        }, "lyric-card").start();
    }

    private static File folder(Context context) {
        File folder = new File(context.getCacheDir(), FOLDER);
        if (!folder.isDirectory() && !folder.mkdirs()) {
            throw new IllegalStateException("no cache folder");
        }
        return folder;
    }

    /** Clear away the pictures drawn a while ago; one being shared right now is newer. */
    private static void sweep(File folder) {
        File[] files = folder.listFiles();
        if (files == null) {
            return;
        }
        long old = System.currentTimeMillis() - KEEP_MS;
        for (File f : files) {
            if (f.lastModified() < old) {
                f.delete();
            }
        }
    }

    /**
     * The card as a bitmap. Its measures are the design's for 1080 pixels on the short
     * side, scaled to the size asked for.
     */
    private static Bitmap draw(Context context, JSONObject card) throws JSONException {
        int w = card.getInt("width"), h = card.getInt("height");
        if (w < 64 || h < 64 || w > 4096 || h > 4096) {
            throw new IllegalStateException("a card of that size can't be drawn");
        }
        float unit = Math.min(w, h) / 1080f;
        int ink = Color.parseColor(card.getString("ink"));
        Bitmap bitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888);
        Canvas canvas = new Canvas(bitmap);

        // The background: the gradient corner to corner, and two faint discs on it.
        Paint fill = new Paint(Paint.ANTI_ALIAS_FLAG);
        fill.setShader(new LinearGradient(0, 0, w, h, Color.parseColor(card.getString("from")), Color.parseColor(card.getString("to")), Shader.TileMode.CLAMP));
        canvas.drawRect(0, 0, w, h, fill);
        Paint disc = new Paint(Paint.ANTI_ALIAS_FLAG);
        disc.setColor(ink);
        disc.setAlpha(22);
        canvas.drawCircle(0.08f * w, 0.02f * h, 420 * unit, disc);
        disc.setAlpha(30);
        canvas.drawCircle(w - 130 * unit, h - 60 * unit, 560 * unit, disc);

        // A story has the sharing app's own bars over its top and bottom: more room there.
        float side = 92 * unit;
        float edge = h > w ? 180 * unit : side;

        // The song, along the bottom: its cover, its title over its artist, the app's mark.
        float coverSide = 148 * unit;
        RectF cover = new RectF(side, h - edge - coverSide, side + coverSide, h - edge);
        drawCover(context, canvas, card.getString("cover"), cover, 28 * unit, ink, unit);

        TextPaint markPaint = paint(ink, 38 * unit, false);
        markPaint.setAlpha(217);
        String mark = "Aoto";
        float markWidth = markPaint.measureText(mark);
        float noteSide = 40 * unit;
        float markLeft = w - side - markWidth - 8 * unit - noteSide;
        drawNote(context, canvas, new RectF(markLeft, cover.centerY() - noteSide / 2, markLeft + noteSide, cover.centerY() + noteSide / 2), ink, 217);
        Paint.FontMetrics metrics = markPaint.getFontMetrics();
        canvas.drawText(mark, w - side - markWidth, cover.centerY() - (metrics.ascent + metrics.descent) / 2, markPaint);

        float nameLeft = cover.right + 28 * unit;
        int nameWidth = Math.max(1, (int) (markLeft - 28 * unit - nameLeft));
        StaticLayout title = oneLine(card.getString("title"), paint(ink, 44 * unit, true), nameWidth);
        TextPaint artistPaint = paint(ink, 38 * unit, false);
        artistPaint.setAlpha(204);
        StaticLayout artist = oneLine(card.getString("artist"), artistPaint, nameWidth);
        float nameTop = cover.centerY() - (title.getHeight() + artist.getHeight() + 6 * unit) / 2;
        drawAt(canvas, title, nameLeft, nameTop);
        drawAt(canvas, artist, nameLeft, nameTop + title.getHeight() + 6 * unit);

        // The lines, in the room above the song: at the first size they fit in.
        JSONArray lines = card.getJSONArray("lines");
        JSONArray sizes = card.getJSONArray("sizes");
        if (lines.length() == 0 || sizes.length() == 0) {
            throw new IllegalStateException("a card with no lines to draw");
        }
        int width = Math.max(1, (int) (w - 2 * side));
        float room = cover.top - 56 * unit - edge;
        List<StaticLayout> block = new ArrayList<>();
        float size = 0, gap = 0, height = 0;
        for (int step = 0; ; step++) {
            // Past the sizes the app listed (four lines longer than any song's), smaller still.
            size = step < sizes.length() ? (float) sizes.getDouble(step) : size * 0.9f;
            TextPaint linePaint = paint(ink, size, true);
            linePaint.setShadowLayer(10 * unit, 0, 2 * unit, 0x30000000);
            gap = size * 0.22f;
            block.clear();
            height = 0;
            for (int i = 0; i < lines.length(); i++) {
                StaticLayout line = wrapped(lines.getString(i), linePaint, width);
                height += line.getHeight() + (i > 0 ? gap : 0);
                block.add(line);
            }
            if (height <= room || size <= SMALLEST) {
                break;
            }
        }
        // A tall card's lines sit in the middle of their room; the others start at the top.
        float y = edge + (h > w ? Math.max(0, (room - height) / 2) : 0);
        canvas.save();
        // Only text that could not fit even at the smallest size is cut, at the room's end.
        canvas.clipRect(0, 0, w, edge + room + 28 * unit);
        for (StaticLayout line : block) {
            drawAt(canvas, line, side, y);
            y += line.getHeight() + gap;
        }
        canvas.restore();
        return bitmap;
    }

    private static TextPaint paint(int color, float size, boolean bold) {
        TextPaint paint = new TextPaint(Paint.ANTI_ALIAS_FLAG | Paint.SUBPIXEL_TEXT_FLAG);
        paint.setColor(color);
        paint.setTextSize(size);
        // The system's sans-serif, with its fallbacks for every script it has letters for.
        paint.setTypeface(Typeface.create("sans-serif", bold ? Typeface.BOLD : Typeface.NORMAL));
        return paint;
    }

    /** One lyric line, wrapped to `width`; it starts on the side its script reads from. */
    private static StaticLayout wrapped(String text, TextPaint paint, int width) {
        return StaticLayout.Builder.obtain(text, 0, text.length(), paint, width)
                .setAlignment(Layout.Alignment.ALIGN_NORMAL)
                .setTextDirection(TextDirectionHeuristics.FIRSTSTRONG_LTR)
                .setLineSpacing(0, 1.12f)
                .setIncludePad(false)
                .setBreakStrategy(Layout.BREAK_STRATEGY_BALANCED)
                .setHyphenationFrequency(Layout.HYPHENATION_FREQUENCY_NONE)
                .build();
    }

    /** A title or an artist: one line, ending in an ellipsis when it is too long, beside the cover. */
    private static StaticLayout oneLine(String text, TextPaint paint, int width) {
        StaticLayout layout = oneLine(text, paint, width, Layout.Alignment.ALIGN_NORMAL);
        // Right-to-left text would start at the far end of its box: keep it by the cover.
        if (layout.getLineCount() > 0 && layout.getParagraphDirection(0) == Layout.DIR_RIGHT_TO_LEFT) {
            layout = oneLine(text, paint, width, Layout.Alignment.ALIGN_OPPOSITE);
        }
        return layout;
    }

    private static StaticLayout oneLine(String text, TextPaint paint, int width, Layout.Alignment alignment) {
        return StaticLayout.Builder.obtain(text, 0, text.length(), paint, width)
                .setAlignment(alignment)
                .setTextDirection(TextDirectionHeuristics.FIRSTSTRONG_LTR)
                .setIncludePad(false)
                .setMaxLines(1)
                .setEllipsize(TextUtils.TruncateAt.END)
                .build();
    }

    private static void drawAt(Canvas canvas, Layout layout, float x, float y) {
        canvas.save();
        canvas.translate(x, y);
        layout.draw(canvas);
        canvas.restore();
    }

    /** The cover, cropped square and round-cornered, with a thin edge; a note when there is none. */
    private static void drawCover(Context context, Canvas canvas, String path, RectF rect, float radius, int ink, float unit) {
        Bitmap cover = path.isEmpty() ? null : decode(path, (int) rect.width());
        Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG | Paint.FILTER_BITMAP_FLAG);
        if (cover != null) {
            float square = Math.min(cover.getWidth(), cover.getHeight());
            float scale = rect.width() / square;
            Matrix place = new Matrix();
            place.setTranslate(-(cover.getWidth() - square) / 2, -(cover.getHeight() - square) / 2);
            place.postScale(scale, scale);
            place.postTranslate(rect.left, rect.top);
            BitmapShader shader = new BitmapShader(cover, Shader.TileMode.CLAMP, Shader.TileMode.CLAMP);
            shader.setLocalMatrix(place);
            paint.setShader(shader);
            canvas.drawRoundRect(rect, radius, radius, paint);
            paint.setShader(null);
        } else {
            paint.setColor(ink);
            paint.setAlpha(40);
            canvas.drawRoundRect(rect, radius, radius, paint);
            float note = rect.width() * 0.5f;
            drawNote(context, canvas, new RectF(rect.centerX() - note / 2, rect.centerY() - note / 2, rect.centerX() + note / 2, rect.centerY() + note / 2), ink, 204);
        }
        paint.setStyle(Paint.Style.STROKE);
        paint.setStrokeWidth(2 * unit);
        paint.setColor(ink);
        paint.setAlpha(64);
        canvas.drawRoundRect(rect, radius, radius, paint);
        if (cover != null) {
            cover.recycle();
        }
    }

    /** The cover's file, read no bigger than it is drawn; null when it can't be read. */
    private static Bitmap decode(String path, int side) {
        BitmapFactory.Options options = new BitmapFactory.Options();
        options.inJustDecodeBounds = true;
        BitmapFactory.decodeFile(path, options);
        if (options.outWidth <= 0 || options.outHeight <= 0) {
            return null;
        }
        int sample = 1;
        while (Math.min(options.outWidth, options.outHeight) / (sample * 2) >= side) {
            sample *= 2;
        }
        options.inJustDecodeBounds = false;
        options.inSampleSize = sample;
        return BitmapFactory.decodeFile(path, options);
    }

    /** The app's music note (the notification's icon) in `ink`. */
    private static void drawNote(Context context, Canvas canvas, RectF rect, int ink, int alpha) {
        Drawable note = ContextCompat.getDrawable(context, R.drawable.ic_stat_music);
        if (note == null) {
            return;
        }
        note = note.mutate();
        note.setTint(ink);
        note.setAlpha(alpha);
        note.setBounds((int) rect.left, (int) rect.top, (int) rect.right, (int) rect.bottom);
        note.draw(canvas);
    }

    /** A picture this class drew: nothing else in the app's storage is saved or shared from here. */
    private static File drawn(Context context, String path) throws IOException {
        File file = new File(path).getCanonicalFile();
        if (!folder(context).getCanonicalFile().equals(file.getParentFile()) || !file.isFile()) {
            throw new IllegalStateException("no picture to send");
        }
        return file;
    }

    /**
     * Copy the picture at `path` into the phone's Pictures as `name`, in a Aoto folder.
     * Android 10 and later take it through the media store, with no permission; before
     * that the file is written there directly, which needs the storage permission
     * (MainActivity asks for it first).
     */
    static void save(final Context context, final String path, final String name, final Storage.Done done) {
        new Thread(() -> {
            try {
                File file = drawn(context, path);
                if (Build.VERSION.SDK_INT >= 29) {
                    ContentResolver resolver = context.getContentResolver();
                    ContentValues values = new ContentValues();
                    values.put(MediaStore.MediaColumns.DISPLAY_NAME, name);
                    values.put(MediaStore.MediaColumns.MIME_TYPE, "image/png");
                    values.put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_PICTURES + "/Aoto");
                    // Hidden from the gallery until it is whole.
                    values.put(MediaStore.MediaColumns.IS_PENDING, 1);
                    Uri item = resolver.insert(MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY), values);
                    if (item == null) {
                        throw new IllegalStateException("can't create the picture");
                    }
                    try (InputStream in = new FileInputStream(file); OutputStream out = resolver.openOutputStream(item)) {
                        if (out == null) {
                            throw new IllegalStateException("can't write the picture");
                        }
                        Storage.copy(in, out);
                    } catch (Exception e) {
                        resolver.delete(item, null, null);
                        throw e;
                    }
                    values.clear();
                    values.put(MediaStore.MediaColumns.IS_PENDING, 0);
                    resolver.update(item, values, null, null);
                } else {
                    File folder = new File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_PICTURES), "Aoto");
                    if (!folder.isDirectory() && !folder.mkdirs()) {
                        throw new IllegalStateException("can't create the Pictures folder");
                    }
                    File target = unused(folder, name);
                    try (InputStream in = new FileInputStream(file); OutputStream out = new FileOutputStream(target)) {
                        Storage.copy(in, out);
                    }
                    // So the gallery finds it.
                    MediaScannerConnection.scanFile(context, new String[] {target.getAbsolutePath()}, new String[] {"image/png"}, null);
                }
                done.finish(true, "Pictures/Aoto");
            } catch (Exception e) {
                done.finish(false, String.valueOf(e.getMessage()));
            }
        }, "lyric-card-save").start();
    }

    /** `name` in `folder`, or "name (2).png" and so on when that is taken. */
    private static File unused(File folder, String name) {
        File file = new File(folder, name);
        int dot = name.lastIndexOf('.');
        String stem = dot > 0 ? name.substring(0, dot) : name, ending = dot > 0 ? name.substring(dot) : "";
        for (int n = 2; file.exists(); n++) {
            file = new File(folder, stem + " (" + n + ")" + ending);
        }
        return file;
    }

    /** Offer the picture at `path` to the phone's other apps, through the share sheet. */
    static void share(Activity activity, String path, Storage.Done done) {
        try {
            Uri uri = FileProvider.getUriForFile(activity, activity.getPackageName() + ".share", drawn(activity, path));
            Intent send = new Intent(Intent.ACTION_SEND)
                    .setType("image/png")
                    .putExtra(Intent.EXTRA_STREAM, uri)
                    .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
            // The sheet's own preview reads the picture through this.
            send.setClipData(ClipData.newRawUri(null, uri));
            activity.startActivity(Intent.createChooser(send, null));
            done.finish(true, "");
        } catch (Exception e) {
            done.finish(false, String.valueOf(e.getMessage()));
        }
    }
}
