package io.github.aminuddinihsan20.bitchord;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * What Android Auto's side decides, apart from Android so it can be checked on a desktop
 * (`sh android/test-java.sh`): whether there is an app to browse, who may browse it, how
 * the app's rows are read, which of its lists changed, and what the car's buttons mean.
 *
 * The music is played by the Rust app, which only runs in the activity's process. A car
 * can browse and play while that process lives (the activity may be away, the screen
 * off); in a process started for the car alone there is nothing to browse, and the car
 * is told to open the app on the phone.
 */
final class AutoLogic {
    /** The tree's root, and its list of what plays next (src/auto.rs names the nodes). */
    static final String ROOT = "root", QUEUE = "queue";

    /** What the car shows when there is no app to play. */
    static final String OPEN_APP = "Open BitChord on your phone to play here";

    /** No app in this process; an app that hasn't published its tree yet; a tree to browse. */
    static final int NO_APP = 0, STARTING = 1, READY = 2;

    /** What a load of rows gets: the rows, a wait for the app, or no rows. */
    static final int SEND = 0, WAIT = 1, NOTHING = 2;

    /** How long a load waits for an app that is starting. */
    static final long PATIENCE_MS = 3000;

    /** The player's commands, as src/platform.rs (MediaCommand) counts them. */
    static final int PLAY = 1, PAUSE = 2, TOGGLE = 3, NEXT = 4, PREVIOUS = 5, SEEK = 6, SHUFFLE = 7, REPEAT = 8;

    /** The car's two extra buttons. */
    static final String ACTION_SHUFFLE = "io.github.aminuddinihsan20.bitchord.SHUFFLE";
    static final String ACTION_REPEAT = "io.github.aminuddinihsan20.bitchord.REPEAT";

    /** What the session says while no player service runs: to open the app, nothing, or the loaded song. */
    static final int SAY_OPEN_APP = 0, SAY_NOTHING = 1, SAY_SONG = 2;

    /** Android Auto and the Google app (the Assistant), for a phone where they are not part of the system. */
    private static final String ANDROID_AUTO = "com.google.android.projection.gearhead";
    private static final String ASSISTANT = "com.google.android.googlequicksearchbox";

    private static final String FIELD = "\u001f";

    private AutoLogic() {
    }

    /** A row of the tree: something to play, or a list to open. */
    static final class Row {
        final String id, title, subtitle;
        final boolean playable;

        Row(String id, String title, String subtitle, boolean playable) {
            this.id = id;
            this.title = title;
            this.subtitle = subtitle;
            this.playable = playable;
        }
    }

    /** What plays, as the app's line says it (NowPlaying::to_line in src/platform.rs). */
    static final class Song {
        String title = "", artist = "", cover = "";
        boolean playing;
        long duration, position;
        /** Its place in the queue, or -1. */
        long queueIndex = -1;
        float speed = 1f;
        /** 0 the queue doesn't repeat, 1 all of it does, 2 this song does. */
        int repeat;
    }

    /** `versions`: what the app has published (empty until it has). */
    static int state(boolean appRunning, String versions) {
        if (!appRunning) {
            return NO_APP;
        }
        return versions == null || versions.isEmpty() ? STARTING : READY;
    }

    /** What a load asked `waitedMs` ago gets now. */
    static int load(int state, long waitedMs) {
        if (state == READY) {
            return SEND;
        }
        return state == STARTING && waitedMs < PATIENCE_MS ? WAIT : NOTHING;
    }

    /**
     * Who may browse: the tree names what the listener plays and keeps. The app itself
     * and the system may, and of other apps those that are part of the system (as
     * Android Auto is on Android 10 and later) and, wherever they come from, Android
     * Auto and the Assistant. `packageIsCallers`: the name the caller gave is a package
     * of its own uid. A debug build lets anything in, for the test tools.
     */
    static boolean allowed(boolean ownUid, boolean systemUid, boolean packageIsCallers, boolean systemApp, String callerPackage, boolean debuggable) {
        if (debuggable || ownUid || systemUid) {
            return true;
        }
        return packageIsCallers && (systemApp || ANDROID_AUTO.equals(callerPackage) || ASSISTANT.equals(callerPackage));
    }

    /**
     * Whether a caller that may browse gets a tree. Not when it only wants the most
     * recent song (`recentOnly`): that is Android's "resume" card, shown once an app is
     * gone and played through this service alone, which can't play.
     */
    static boolean serves(boolean recentOnly) {
        return !recentOnly;
    }

    /** The app's rows (auto::encode in src/auto.rs): one a line, id, title, subtitle and "p" or "b". */
    static List<Row> rows(String lines) {
        List<Row> rows = new ArrayList<>();
        if (lines == null || lines.isEmpty()) {
            return rows;
        }
        for (String line : lines.split("\n")) {
            String[] f = line.split(FIELD, -1);
            if (f.length >= 4 && !f[0].isEmpty()) {
                rows.add(new Row(f[0], f[1], f[2], "p".equals(f[3])));
            }
        }
        return rows;
    }

    private static Map<String, String> counts(String versions) {
        Map<String, String> counts = new LinkedHashMap<>();
        if (versions != null && !versions.isEmpty()) {
            for (String line : versions.split("\n")) {
                String[] f = line.split(FIELD, -1);
                if (f.length == 2) {
                    counts.put(f[0], f[1]);
                }
            }
        }
        return counts;
    }

    /** The nodes whose rows changed between two looks at the app's counts ("node␟count" a line). */
    static List<String> changed(String before, String now) {
        Map<String, String> was = counts(before), is = counts(now);
        List<String> changed = new ArrayList<>();
        for (Map.Entry<String, String> node : is.entrySet()) {
            if (!node.getValue().equals(was.get(node.getKey()))) {
                changed.add(node.getKey());
            }
        }
        for (String node : was.keySet()) {
            if (!is.containsKey(node)) {
                changed.add(node);
            }
        }
        return changed;
    }

    /** A queue row's place in the queue ("queue/<place>/<song>"), which is its id in the session's queue; -1 for any other row. */
    static long queueId(String mediaId) {
        if (mediaId == null || !mediaId.startsWith("queue/")) {
            return -1;
        }
        int end = mediaId.indexOf('/', 6);
        if (end < 0 || end == mediaId.length() - 1) {
            return -1;
        }
        try {
            return Long.parseLong(mediaId.substring(6, end));
        } catch (NumberFormatException e) {
            return -1;
        }
    }

    /** The player's command for one of the car's extra buttons, or 0. */
    static int command(String customAction) {
        if (ACTION_SHUFFLE.equals(customAction)) {
            return SHUFFLE;
        }
        return ACTION_REPEAT.equals(customAction) ? REPEAT : 0;
    }

    /** What the repeat button is called: how the queue repeats now. A press moves it on. */
    static String repeatLabel(int repeat) {
        return repeat == 1 ? "Repeating all" : repeat == 2 ? "Repeating one song" : "Repeat is off";
    }

    /** What the session says while no player service runs to say it. */
    static int idle(int state, boolean songLoaded) {
        if (state == NO_APP) {
            return SAY_OPEN_APP;
        }
        return songLoaded ? SAY_SONG : SAY_NOTHING;
    }

    /** What the app was opened for: nothing more, to play, or to search for what was said and play it. */
    static final int OPENED = 0, OPENED_TO_PLAY = 1, OPENED_TO_SEARCH = 2;

    /** "Play … on BitChord", said to the Assistant with no car connected: Android opens the app with it. */
    private static final String VOICE_SEARCH = "android.media.action.MEDIA_PLAY_FROM_SEARCH";

    /**
     * What the intent the activity was opened with asks for. `playAction`: a widget's
     * Play. `fromHistory`: Android reopened the app from the recent apps, with the intent
     * it was first started with, which is not asking again.
     */
    static int opening(String action, String playAction, boolean fromHistory) {
        if (fromHistory || action == null) {
            return OPENED;
        }
        if (action.equals(VOICE_SEARCH)) {
            return OPENED_TO_SEARCH;
        }
        return action.equals(playAction) ? OPENED_TO_PLAY : OPENED;
    }

    /** The app's line of what plays, or null when nothing is loaded. Fields added later may be missing. */
    static Song song(String line) {
        if (line == null || line.isEmpty()) {
            return null;
        }
        String[] f = line.split(FIELD, -1);
        if (f.length < 6) {
            return null;
        }
        Song song = new Song();
        song.title = f[0];
        song.artist = f[1];
        song.cover = f[2];
        song.playing = "1".equals(f[3]);
        song.duration = number(f[4], 0);
        song.position = number(f[5], 0);
        long hundredths = f.length > 6 ? number(f[6], 0) : 0;
        song.speed = hundredths > 0 ? hundredths / 100f : 1f;
        song.repeat = f.length > 7 ? (int) number(f[7], 0) : 0;
        song.queueIndex = f.length > 8 ? number(f[8], -1) : -1;
        return song;
    }

    private static long number(String text, long otherwise) {
        try {
            return Long.parseLong(text);
        } catch (NumberFormatException e) {
            return otherwise;
        }
    }
}
