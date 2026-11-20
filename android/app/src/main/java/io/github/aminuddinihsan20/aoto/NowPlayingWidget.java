package io.github.aminuddinihsan20.aoto;

import android.app.PendingIntent;
import android.appwidget.AppWidgetManager;
import android.appwidget.AppWidgetProvider;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.res.Configuration;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.PorterDuff;
import android.graphics.PorterDuffXfermode;
import android.graphics.Rect;
import android.graphics.drawable.Drawable;
import android.os.Build;
import android.os.Bundle;
import android.util.DisplayMetrics;
import android.util.Log;
import android.view.View;
import android.widget.RemoteViews;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;

/**
 * The home-screen widgets: what plays and its buttons, in five shapes (4×2 with the
 * cover, the progress, the times and three buttons; 2×2, the cover with play; 4×1, a
 * pill; 2×1, a smaller pill; 4×3, the song and what is up next). They show what
 * PlaybackService shows and change when it says so: no timer of their own, nothing that
 * wakes the phone. With no song there, or no app running, they say that nothing is
 * playing, and Play opens the app to play. What a tap does, and what fits in the cell a
 * launcher gives, is decided in WidgetLogic.
 *
 * Everything is drawn on one thread of its own, in the order asked: covers are scaled
 * there, and the main thread only hands over what to show.
 */
public class NowPlayingWidget extends AppWidgetProvider {
    /** 4×2. */
    public static class Wide extends NowPlayingWidget {
    }

    /** 2×2. */
    public static class Cover extends NowPlayingWidget {
    }

    /** 4×1. */
    public static class Pill extends NowPlayingWidget {
    }

    /** 2×1. */
    public static class Mini extends NowPlayingWidget {
    }

    /** 4×3. */
    public static class Queue extends NowPlayingWidget {
    }

    /** Opens MainActivity to play: what a widget's Play does with no player to tell. */
    static final String ACTION_PLAY = "io.github.aminuddinihsan20.aoto.OPEN_AND_PLAY";

    private static final String TAG = "aoto";

    /** The request codes of the queue widget's rows' intents start here (the buttons' are their controls' numbers). */
    private static final int ROW_REQUESTS = 10;

    /** A song as the service published it. */
    private static final class Song {
        final String title, artist;
        final boolean playing;
        /** Percent played, or WidgetLogic.NO_PROGRESS. */
        final int percent;
        /** How far it was, of how long (ms; 0: not known), when the system's clock (elapsedRealtime) said `at`. */
        final long position, duration, at;
        /** How fast it plays: 1 as recorded. */
        final float speed;
        /** Its place in the queue, or -1. */
        final long queueIndex;
        /** As loaded for the notification, or null. */
        final Bitmap cover;

        Song(AutoLogic.Song song, int percent, long at, Bitmap cover) {
            this(song.title, song.artist, song.playing, percent, song.position, song.duration, at, song.speed, song.queueIndex, cover);
        }

        private Song(String title, String artist, boolean playing, int percent, long position, long duration, long at, float speed, long queueIndex, Bitmap cover) {
            this.title = title;
            this.artist = artist;
            this.playing = playing;
            this.percent = percent;
            this.position = position;
            this.duration = duration;
            this.at = at;
            this.speed = speed;
            this.queueIndex = queueIndex;
            this.cover = cover;
        }

        /** The same song, its bar moved on. */
        Song at(int percent) {
            return new Song(title, artist, playing, percent, position, duration, at, speed, queueIndex, cover);
        }
    }

    private static final ExecutorService worker = Executors.newSingleThreadExecutor();

    /** Is a queue widget on the home screen, as of the last time all were drawn? Then the service reads the queue for it. */
    private static volatile boolean queueShown;

    // The worker's own: touched on its thread only.
    /** What the widgets show; null: no song. */
    private static Song song;
    /** The app's "Up next" (the song that plays first), for the queue widget. */
    private static List<AutoLogic.Row> upNext = new ArrayList<>();
    /** The widgets with a progress bar (4×2, 4×3) on the home screen, as of the last time all were drawn. */
    private static int[] wideIds = new int[0], queueIds = new int[0];
    /** `artOf` (a cover, or null for the placeholder) in the sizes the widgets asked for. */
    private static final HashMap<String, Bitmap> arts = new HashMap<>();
    private static Bitmap artOf;

    /** The service: this song is what plays (or is paused) now, `percent` played, as of `at` on the system's clock. */
    static void show(Context context, AutoLogic.Song playing, int percent, long at, Bitmap cover) {
        Song now = new Song(playing, percent, at, cover);
        draw(context, () -> song = now);
    }

    /** The service: the song has moved on to this percent. Only the 4×2 and 4×3 widgets have a bar. */
    static void progress(Context context, int percent) {
        Context app = context.getApplicationContext();
        worker.execute(() -> {
            if (song == null) {
                return;
            }
            song = song.at(percent);
            try {
                AppWidgetManager manager = AppWidgetManager.getInstance(app);
                if (wideIds.length > 0) {
                    RemoteViews bar = new RemoteViews(app.getPackageName(), R.layout.widget_wide);
                    progress(bar);
                    manager.partiallyUpdateAppWidget(wideIds, bar);
                }
                if (queueIds.length > 0) {
                    RemoteViews bar = new RemoteViews(app.getPackageName(), R.layout.widget_queue);
                    progress(bar);
                    manager.partiallyUpdateAppWidget(queueIds, bar);
                }
            } catch (RuntimeException e) {
                Log.w(TAG, "widget progress not shown", e);
            }
        });
    }

    /** Whether the home screen has a queue widget: the service then keeps `queue` told. */
    static boolean showsQueue() {
        return queueShown;
    }

    /** The service: the app's "Up next" changed. Only the queue widget shows it. */
    static void queue(Context context, List<AutoLogic.Row> rows) {
        Context app = context.getApplicationContext();
        worker.execute(() -> {
            upNext = rows;
            try {
                AppWidgetManager manager = AppWidgetManager.getInstance(app);
                for (int id : queueIds) {
                    manager.updateAppWidget(id, queue(app, size(app, manager, id)));
                }
            } catch (RuntimeException e) {
                Log.w(TAG, "queue widget not drawn", e);
            }
        });
    }

    /** No song any more, or no player to show one. */
    static void clear(Context context) {
        draw(context, () -> {
            song = null;
            upNext = new ArrayList<>();
        });
    }

    /**
     * The same, for a process about to end: waits until the launcher has been told (it
     * keeps showing a widget as it was last drawn, buttons and all).
     */
    static void clearNow(Context context) {
        Context app = context.getApplicationContext();
        try {
            worker.submit(() -> {
                song = null;
                drawAll(app);
            }).get(500, TimeUnit.MILLISECONDS);
        } catch (Exception e) {
            // Ending anyway; a button left behind still opens the app (PlaybackService).
        }
    }

    /** Draw the widgets as they should be now: at the app's start, whatever an earlier run left. */
    static void refresh(Context context) {
        draw(context, () -> {
        });
    }

    private static void draw(Context context, Runnable change) {
        Context app = context.getApplicationContext();
        worker.execute(() -> {
            change.run();
            drawAll(app);
        });
    }

    /** A widget was added, resized or removed, or the system asks for all of them again. */
    private void redraw(Context context) {
        Context app = context.getApplicationContext();
        PendingResult pending = goAsync();
        worker.execute(() -> {
            try {
                drawAll(app);
            } finally {
                pending.finish();
            }
        });
    }

    @Override
    public void onUpdate(Context context, AppWidgetManager manager, int[] ids) {
        redraw(context);
    }

    @Override
    public void onAppWidgetOptionsChanged(Context context, AppWidgetManager manager, int id, Bundle options) {
        redraw(context);
    }

    @Override
    public void onDeleted(Context context, int[] ids) {
        redraw(context);
    }

    private static void drawAll(Context c) {
        try {
            AppWidgetManager manager = AppWidgetManager.getInstance(c);
            Bitmap cover = song == null ? null : song.cover;
            if (cover != artOf) {
                arts.clear();
                artOf = cover;
            }
            wideIds = manager.getAppWidgetIds(new ComponentName(c, Wide.class));
            for (int id : wideIds) {
                manager.updateAppWidget(id, wide(c, size(c, manager, id)));
            }
            for (int id : manager.getAppWidgetIds(new ComponentName(c, Cover.class))) {
                manager.updateAppWidget(id, cover(c, size(c, manager, id)));
            }
            for (int id : manager.getAppWidgetIds(new ComponentName(c, Pill.class))) {
                manager.updateAppWidget(id, pill(c));
            }
            for (int id : manager.getAppWidgetIds(new ComponentName(c, Mini.class))) {
                manager.updateAppWidget(id, mini(c, size(c, manager, id)));
            }
            queueIds = manager.getAppWidgetIds(new ComponentName(c, Queue.class));
            queueShown = queueIds.length > 0;
            for (int id : queueIds) {
                manager.updateAppWidget(id, queue(c, size(c, manager, id)));
            }
        } catch (RuntimeException e) {
            // A phone without widgets, or a launcher that refused the update.
            Log.w(TAG, "widgets not drawn", e);
        }
    }

    /** The widget's size in dp as the launcher reports it for the way the screen is turned; 0s when it doesn't. */
    private static int[] size(Context c, AppWidgetManager manager, int id) {
        Bundle options = manager.getAppWidgetOptions(id);
        boolean upright = c.getResources().getConfiguration().orientation != Configuration.ORIENTATION_LANDSCAPE;
        return new int[] {
                options.getInt(upright ? AppWidgetManager.OPTION_APPWIDGET_MIN_WIDTH : AppWidgetManager.OPTION_APPWIDGET_MAX_WIDTH),
                options.getInt(upright ? AppWidgetManager.OPTION_APPWIDGET_MAX_HEIGHT : AppWidgetManager.OPTION_APPWIDGET_MIN_HEIGHT),
        };
    }

    // A widget keeps what an earlier update set: each of these sets everything that can differ.

    private static RemoteViews wide(Context c, int[] size) {
        RemoteViews v = new RemoteViews(c.getPackageName(), R.layout.widget_wide);
        int side = px(c, 16), ends = px(c, WidgetLogic.widePadDp(size[1]));
        v.setViewPadding(android.R.id.background, side, ends, side, ends);
        // The view is as large as its picture: the size is chosen here.
        int cover = side(c, px(c, WidgetLogic.wideCoverDp(size[0], size[1])));
        v.setImageViewBitmap(R.id.widget_art, art(c, cover, cover, c.getResources().getDimension(R.dimen.widget_inner_radius)));
        // With a song, the player; without one, the invitation to play.
        v.setViewVisibility(R.id.widget_playing, song != null ? View.VISIBLE : View.GONE);
        v.setViewVisibility(R.id.widget_empty, song != null ? View.GONE : View.VISIBLE);
        int twoLines = WidgetLogic.roomForArtist(size[1]) ? View.VISIBLE : View.GONE;
        v.setViewVisibility(R.id.widget_artist, twoLines);
        v.setViewVisibility(R.id.widget_empty_text, twoLines);
        texts(c, v);
        progress(v);
        times(v, WidgetLogic.roomForTimes(size[1]));
        buttons(c, v, true, true);
        toggleShape(v);
        control(c, v, R.id.widget_play_something, WidgetLogic.TOGGLE);
        return v;
    }

    private static RemoteViews cover(Context c, int[] size) {
        RemoteViews v = new RemoteViews(c.getPackageName(), R.layout.widget_cover);
        texts(c, v);
        boolean has = song != null && song.cover != null;
        // Without a cover the background, with the Drop, is the picture.
        v.setViewVisibility(R.id.widget_art, has ? View.VISIBLE : View.INVISIBLE);
        if (has) {
            int longest = Math.max(size[0], size[1]);
            int side = side(c, px(c, longest > 0 ? longest : 180));
            if (Build.VERSION.SDK_INT >= 31) {
                // The view crops it to its shape, and the widget's outline rounds it.
                v.setImageViewBitmap(R.id.widget_art, art(c, side, side, 0));
            } else {
                // Nothing clips there: the picture gets the widget's own shape and corners.
                int[] box = WidgetLogic.fit(size[0], size[1], side);
                float shown = size[0] > 0 ? (float) box[0] / px(c, size[0]) : 1;
                v.setImageViewBitmap(R.id.widget_art, art(c, box[0], box[1], c.getResources().getDimension(R.dimen.widget_radius) * shown));
            }
        }
        buttons(c, v, false, false);
        toggleShape(v);
        return v;
    }

    private static RemoteViews pill(Context c) {
        RemoteViews v = new RemoteViews(c.getPackageName(), R.layout.widget_pill);
        texts(c, v);
        int side = side(c, px(c, 48));
        v.setImageViewBitmap(R.id.widget_art, art(c, side, side, side / 2f));
        buttons(c, v, false, true);
        return v;
    }

    private static RemoteViews mini(Context c, int[] size) {
        RemoteViews v = new RemoteViews(c.getPackageName(), R.layout.widget_mini);
        // The view is as large as its picture; in a cell too narrow for it there is none.
        int cover = WidgetLogic.miniCoverDp(size[0]);
        v.setViewVisibility(R.id.widget_art, cover > 0 ? View.VISIBLE : View.GONE);
        if (cover > 0) {
            int side = side(c, px(c, cover));
            v.setImageViewBitmap(R.id.widget_art, art(c, side, side, side / 2f));
        }
        buttons(c, v, false, true);
        toggleShape(v);
        return v;
    }

    private static RemoteViews queue(Context c, int[] size) {
        RemoteViews v = new RemoteViews(c.getPackageName(), R.layout.widget_queue);
        texts(c, v);
        int side = side(c, px(c, 64));
        v.setImageViewBitmap(R.id.widget_art, art(c, side, side, px(c, 16)));
        progress(v);
        buttons(c, v, false, false);
        toggleShape(v);
        // The songs after the one that plays, as rows of their own: as many as fit.
        boolean live = WidgetLogic.tap(WidgetLogic.ROW, song != null) == WidgetLogic.COMMAND;
        int room = WidgetLogic.queueRows(size[1]);
        List<AutoLogic.Row> next = live ? WidgetLogic.upNext(upNext, song.queueIndex, room) : new ArrayList<>();
        v.setViewVisibility(R.id.widget_up_next, next.isEmpty() ? View.GONE : View.VISIBLE);
        v.setViewVisibility(R.id.widget_queue_end, live && room > 0 && next.isEmpty() ? View.VISIBLE : View.GONE);
        v.removeAllViews(R.id.widget_rows);
        for (int at = 0; at < next.size(); at++) {
            AutoLogic.Row entry = next.get(at);
            RemoteViews row = new RemoteViews(c.getPackageName(), R.layout.widget_queue_row);
            row.setTextViewText(R.id.widget_row_title, entry.title);
            row.setTextViewText(R.id.widget_row_artist, entry.subtitle);
            // One intent a row of the widget, told anew which song the row holds.
            row.setOnClickPendingIntent(R.id.widget_row, PendingIntent.getForegroundService(c, ROW_REQUESTS + at,
                    PlaybackService.widgetRow(c, entry.id), PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT));
            v.addView(R.id.widget_rows, row);
        }
        return v;
    }

    private static void texts(Context c, RemoteViews v) {
        v.setTextViewText(R.id.widget_title, song == null ? c.getString(R.string.widget_empty_title) : song.title);
        v.setTextViewText(R.id.widget_artist, song == null ? c.getString(R.string.widget_empty_text) : song.artist);
    }

    private static void progress(RemoteViews v) {
        int percent = song == null ? WidgetLogic.NO_PROGRESS : song.percent;
        v.setViewVisibility(R.id.widget_progress, percent == WidgetLogic.NO_PROGRESS ? View.INVISIBLE : View.VISIBLE);
        v.setProgressBar(R.id.widget_progress, 100, Math.max(percent, 0), false);
    }

    /**
     * The times under the 4×2 widget's bar. The time played counts by itself, in the
     * launcher, from where the song was when the service last said (it says again at
     * every pause, seek and song): nothing is sent each second. See WidgetLogic.elapsed.
     */
    private static void times(RemoteViews v, boolean room) {
        boolean known = room && song != null && song.duration > 0;
        v.setViewVisibility(R.id.widget_times, known ? View.VISIBLE : View.GONE);
        int how = known ? WidgetLogic.elapsed(song.playing, song.speed) : WidgetLogic.NO_TIME;
        v.setViewVisibility(R.id.widget_elapsed_counting, how == WidgetLogic.COUNTS ? View.VISIBLE : View.GONE);
        v.setViewVisibility(R.id.widget_elapsed, how == WidgetLogic.STANDS ? View.VISIBLE : View.GONE);
        v.setChronometer(R.id.widget_elapsed_counting, known ? song.at - song.position : 0, null, how == WidgetLogic.COUNTS);
        if (known) {
            v.setTextViewText(R.id.widget_elapsed, WidgetLogic.time(song.position));
            v.setTextViewText(R.id.widget_total, WidgetLogic.time(song.duration));
        }
    }

    /** Pause, while a song plays, is squarer; Play is fully round. */
    private static void toggleShape(RemoteViews v) {
        boolean playing = song != null && song.playing;
        v.setInt(R.id.widget_toggle, "setBackgroundResource", playing ? R.drawable.widget_button_square : R.drawable.widget_button_primary);
    }

    private static void buttons(Context c, RemoteViews v, boolean previous, boolean next) {
        boolean playing = song != null && song.playing;
        v.setImageViewResource(R.id.widget_toggle, playing ? R.drawable.ic_widget_pause : R.drawable.ic_widget_play);
        v.setContentDescription(R.id.widget_toggle, c.getString(playing ? R.string.widget_pause : R.string.widget_play));
        control(c, v, android.R.id.background, WidgetLogic.BODY);
        control(c, v, R.id.widget_toggle, WidgetLogic.TOGGLE);
        if (previous) {
            control(c, v, R.id.widget_previous, WidgetLogic.PREVIOUS);
        }
        if (next) {
            control(c, v, R.id.widget_next, WidgetLogic.NEXT);
        }
    }

    private static void control(Context c, RemoteViews v, int view, int control) {
        int tap = WidgetLogic.tap(control, song != null);
        v.setViewVisibility(view, tap == WidgetLogic.HIDDEN ? View.GONE : View.VISIBLE);
        if (tap == WidgetLogic.HIDDEN) {
            return;
        }
        int flags = PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT;
        PendingIntent pending;
        if (tap == WidgetLogic.COMMAND) {
            // "Foreground": the kind of start Android allows from a widget whatever state the
            // app is in. The service is the one that already runs, showing its notification.
            pending = PendingIntent.getForegroundService(c, control, PlaybackService.widgetButton(c, control), flags);
        } else if (tap == WidgetLogic.OPEN_AND_PLAY) {
            pending = PendingIntent.getActivity(c, 1, new Intent(c, MainActivity.class).setAction(ACTION_PLAY), flags);
        } else {
            pending = PendingIntent.getActivity(c, 0, new Intent(c, MainActivity.class), PendingIntent.FLAG_IMMUTABLE);
        }
        v.setOnClickPendingIntent(view, pending);
    }

    private static int px(Context c, int dp) {
        return Math.round(dp * c.getResources().getDisplayMetrics().density);
    }

    /** The side of a cover shown this wide, within what a widget's update may carry. */
    private static int side(Context c, int wantPx) {
        DisplayMetrics screen = c.getResources().getDisplayMetrics();
        return WidgetLogic.coverSide(wantPx, (long) screen.widthPixels * screen.heightPixels);
    }

    /**
     * The song's cover, or without one the placeholder, cut to this many pixels. From
     * Android 12 on the views round their pictures themselves; below it the corners are
     * rounded here, by `radius`.
     */
    private static Bitmap art(Context c, int w, int h, float radius) {
        if (Build.VERSION.SDK_INT >= 31) {
            radius = 0;
        }
        String key = w + "x" + h + "r" + Math.round(radius);
        Bitmap art = arts.get(key);
        if (art != null) {
            return art;
        }
        // A widget being resized asks for size after size.
        if (arts.size() >= 8) {
            arts.clear();
        }
        art = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888);
        art.setDensity(c.getResources().getDisplayMetrics().densityDpi);
        Canvas canvas = new Canvas(art);
        Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG | Paint.FILTER_BITMAP_FLAG);
        if (radius > 0) {
            // The shape first; what is drawn next stays only where the shape is.
            canvas.drawRoundRect(0, 0, w, h, radius, radius, paint);
            paint.setXfermode(new PorterDuffXfermode(PorterDuff.Mode.SRC_IN));
        }
        if (artOf != null) {
            int[] part = WidgetLogic.crop(artOf.getWidth(), artOf.getHeight(), w, h);
            canvas.drawBitmap(artOf, new Rect(part[0], part[1], part[0] + part[2], part[1] + part[3]), new Rect(0, 0, w, h), paint);
        } else {
            Drawable placeholder = c.getDrawable(R.drawable.widget_art_empty);
            placeholder.setBounds(0, 0, w, h);
            canvas.saveLayer(0, 0, w, h, paint);
            placeholder.draw(canvas);
            canvas.restore();
        }
        arts.put(key, art);
        return art;
    }
}
