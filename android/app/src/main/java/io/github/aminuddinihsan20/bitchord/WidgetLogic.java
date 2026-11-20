package io.github.aminuddinihsan20.bitchord;

/**
 * What the home-screen widgets decide, apart from Android so it can be checked on a
 * desktop (`sh android/test-java.sh`): what a tap does, when the progress bar moves, and
 * how large a cover may be.
 */
final class WidgetLogic {
    /** A widget's controls. */
    static final int BODY = 0, TOGGLE = 1, PREVIOUS = 2, NEXT = 3;

    /** What a tap does: open the app, open it and play, tell the player, or (not shown) nothing. */
    static final int OPEN = 0, OPEN_AND_PLAY = 1, COMMAND = 2, HIDDEN = 3;

    /** A song's length isn't known: the bar is left out. */
    static final int NO_PROGRESS = -1;

    /** The bar moves no more often than this. */
    static final long PROGRESS_EVERY_MS = 2000;

    /** No cover is made larger than this, however dense the screen. */
    static final int MAX_COVER_PX = 512;

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
     * The side of the 4×2 widget's cover, in dp, for the size the launcher gave it (0:
     * not told). The layout (widget_wide.xml) has 12dp of padding all round, 12dp between
     * the cover and the rest, and three 48dp buttons 6dp apart that must stay whole.
     */
    static int wideCoverDp(int widthDp, int heightDp) {
        if (widthDp <= 0 || heightDp <= 0) {
            return 96;
        }
        int beside = 24 + 12 + 3 * 48 + 2 * 6;
        return Math.max(48, Math.min(148, Math.min(heightDp - 24, widthDp - beside)));
    }

    /**
     * Whether the 4×2 widget is high enough (dp; 0: not told) for the artist under the
     * title: 24dp of padding, a 21dp title, a 20dp artist, the bar's 14dp, 48dp buttons.
     */
    static boolean roomForArtist(int heightDp) {
        return heightDp <= 0 || heightDp > 24 + 21 + 20 + 14 + 48;
    }
}
