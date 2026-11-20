package io.github.aminuddinihsan20.aoto;

import java.util.ArrayList;
import java.util.List;

/**
 * What the home-screen widgets decide, apart from Android so it can be checked on a
 * desktop (`sh android/test-java.sh`): what a tap does, when the progress bar moves, how
 * the times read, which songs are up next, and what fits in the cell a launcher gives.
 */
final class WidgetLogic {
    /** A widget's controls; ROW is a song of the queue widget's "Up next". */
    static final int BODY = 0, TOGGLE = 1, PREVIOUS = 2, NEXT = 3, ROW = 4;

    /** What a tap does: open the app, open it and play, tell the player, or (not shown) nothing. */
    static final int OPEN = 0, OPEN_AND_PLAY = 1, COMMAND = 2, HIDDEN = 3;

    /** A song's length isn't known: the bar is left out. */
    static final int NO_PROGRESS = -1;

    /** The bar moves no more often than this. */
    static final long PROGRESS_EVERY_MS = 2000;

    /** No cover is made larger than this, however dense the screen. */
    static final int MAX_COVER_PX = 512;

    /** The time played: not shown, counting on by itself, or standing still. */
    static final int NO_TIME = 0, COUNTS = 1, STANDS = 2;

    /** The queue widget never shows more songs than this, however high it is. */
    static final int MAX_QUEUE_ROWS = 6;

    private WidgetLogic() {
    }

    /**
     * What a control does. `live`: the player (the service) shows a song, so its buttons
     * can be told to it. Without one there is nothing to skip, and Play can only open the
     * app, which plays last time's song: music is played by the app, which runs in its
     * activity, never by the service alone.
     */
    static int tap(int control, boolean live) {
        if (control == BODY) {
            return OPEN;
        }
        if (live) {
            return COMMAND;
        }
        // No song: no rows either.
        return control == TOGGLE ? OPEN_AND_PLAY : HIDDEN;
    }

    /**
     * A button pressed on a widget that still shows a song from an app that has died
     * since (killed without a chance to say so): there is no player to tell.
     */
    static int coldTap(int control) {
        return control == TOGGLE ? OPEN_AND_PLAY : OPEN;
    }

    /** How far the song is, in percent, or NO_PROGRESS. */
    static int percent(long positionMs, long durationMs) {
        if (durationMs <= 0) {
            return NO_PROGRESS;
        }
        long position = Math.max(0, Math.min(positionMs, durationMs));
        return (int) (position * 100 / durationMs);
    }

    /** Whether the bar, showing `shown`, is to be moved to `percent`, `sinceMs` after it last moved. */
    static boolean progressDue(int shown, int percent, long sinceMs) {
        return percent != NO_PROGRESS && percent != shown && sinceMs >= PROGRESS_EVERY_MS;
    }

    /**
     * The side of a cover shown `wantPx` wide, in pixels. The system refuses a widget
     * update whose bitmaps take more than 6 bytes a screen pixel; a cover (4 bytes a
     * pixel) is kept to a sixth of that.
     */
    static int coverSide(int wantPx, long screenPixels) {
        int allowed = (int) Math.sqrt(screenPixels / 4.0);
        return Math.max(1, Math.min(wantPx, Math.min(allowed, MAX_COVER_PX)));
    }

    /**
     * The part of a picture that fills a box of this shape, from its middle: left, top,
     * width, height. A box without a size counts as square.
     */
    static int[] crop(int srcW, int srcH, int boxW, int boxH) {
        if (boxW <= 0 || boxH <= 0) {
            boxW = 1;
            boxH = 1;
        }
        int w = srcW, h = srcH;
        if ((long) srcW * boxH > (long) srcH * boxW) {
            w = (int) ((long) srcH * boxW / boxH);
        } else {
            h = (int) ((long) srcW * boxH / boxW);
        }
        return new int[] {(srcW - w) / 2, (srcH - h) / 2, w, h};
    }

    /** A box's shape with its longer side `side` long: width, height. Square without a size. */
    static int[] fit(int boxW, int boxH, int side) {
        if (boxW <= 0 || boxH <= 0) {
            return new int[] {side, side};
        }
        if (boxW >= boxH) {
            return new int[] {side, Math.max(1, (int) ((long) side * boxH / boxW))};
        }
        return new int[] {Math.max(1, (int) ((long) side * boxW / boxH)), side};
    }

    /**
     * The 4×2 widget's padding, in dp, for the height the launcher gave it (0: not told):
     * the designed 16dp where everything fits with it, 12dp in a lower cell.
     */
    static int widePadDp(int heightDp) {
        return heightDp <= 0 || heightDp >= 152 ? 16 : 12;
    }

    /**
     * The side of the 4×2 widget's cover, in dp, for the size the launcher gave it (0:
     * not told). The layout (widget_wide.xml) has 16dp of padding at the sides, 16dp
     * between the cover and the rest, and three buttons 8dp apart that must stay 48dp wide.
     */
    static int wideCoverDp(int widthDp, int heightDp) {
        if (widthDp <= 0 || heightDp <= 0) {
            return 96;
        }
        int beside = 32 + 16 + 3 * 48 + 2 * 8;
        return Math.max(40, Math.min(148, Math.min(heightDp - 2 * widePadDp(heightDp), widthDp - beside)));
    }

    /**
     * Whether the 4×2 widget is high enough (dp; 0: not told) for the artist under the
     * title: 24dp of padding, a 22dp title, a 20dp artist, the bar's 12dp, 48dp buttons.
     */
    static boolean roomForArtist(int heightDp) {
        return heightDp <= 0 || heightDp >= 24 + 22 + 20 + 12 + 48;
    }

    /** And for the times under the bar as well, 18dp more: the artist comes first. */
    static boolean roomForTimes(int heightDp) {
        return heightDp <= 0 || heightDp >= 24 + 22 + 20 + 12 + 18 + 48;
    }

    /**
     * A time as the widgets write it: "04:02", "1:02:03". The way Android's own counting
     * label (Chronometer) writes the time played, so the two beside each other agree.
     */
    static String time(long ms) {
        long seconds = Math.max(0, ms) / 1000;
        long hours = seconds / 3600, minutes = seconds / 60 % 60;
        String rest = (minutes < 10 ? "0" : "") + minutes + ":" + (seconds % 60 < 10 ? "0" : "") + seconds % 60;
        return hours > 0 ? hours + ":" + rest : rest;
    }

    /**
     * How the time played is shown. A widget can't be told every second, but the
     * launcher can count for itself from a starting point, a second a second: right
     * while the song plays as recorded. Played faster or slower the count would be
     * wrong, and the time is left out; paused, it stands as text.
     */
    static int elapsed(boolean playing, float speed) {
        if (!playing) {
            return STANDS;
        }
        return speed == 1f ? COUNTS : NO_TIME;
    }

    /**
     * The side of the 2×1 widget's cover, in dp, for the width the launcher gave it (0:
     * not told); 0: no cover. Beside it (widget_mini.xml): 24dp of padding, the 52dp
     * play button, Next's 48dp, and 4dp either side of the play button.
     */
    static int miniCoverDp(int widthDp) {
        if (widthDp <= 0) {
            return 48;
        }
        int room = widthDp - (24 + 52 + 48 + 8);
        return room >= 48 ? 48 : room >= 32 ? room : 0;
    }

    /**
     * How many songs of "Up next" the queue widget has room for at this height (dp; 0:
     * not told, the designed three). Above them (widget_queue.xml) are 124dp: the
     * padding, the song that plays, the bar and the heading; a row is 48dp, and 8dp
     * stay free below.
     */
    static int queueRows(int heightDp) {
        if (heightDp <= 0) {
            return 3;
        }
        return Math.max(0, Math.min(MAX_QUEUE_ROWS, (heightDp - 124 - 8) / 48));
    }

    /**
     * The songs after the one that plays, at most `room` of them. `rows`: the app's "Up
     * next" as a car gets it (src/auto.rs), which starts with the song that plays;
     * `playing`: that song's place in the queue, or -1 when it isn't known. A list read
     * before the player moved on never offers the song that plays now, or one before it.
     */
    static List<AutoLogic.Row> upNext(List<AutoLogic.Row> rows, long playing, int room) {
        List<AutoLogic.Row> next = new ArrayList<>();
        boolean first = true;
        for (AutoLogic.Row row : rows) {
            long place = AutoLogic.queueId(row.id);
            boolean after = playing >= 0 ? place > playing : !first;
            first = false;
            if (place >= 0 && after && next.size() < room) {
                next.add(row);
            }
        }
        return next;
    }
}
