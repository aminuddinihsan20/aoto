package io.github.aminuddinihsan20.bitchord;

import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.media.MediaDescription;
import android.media.MediaMetadata;
import android.media.session.MediaSession;
import android.media.session.PlaybackState;
import android.os.Bundle;

import java.util.ArrayList;
import java.util.List;

/**
 * The app's one media session: what the lock screen, a headset's buttons, the media
 * notification and a car (Android Auto) show and control. It belongs to the process
 * rather than to one service, because a car needs it before any song has played: the
 * player service (while a song is loaded) and the car's browser service (while a car is
 * connected) both hold it, and it goes when neither does. Everything here runs on the
 * main thread.
 */
final class PlayerSession {
    // Implemented in android/rust/src/lib.rs.
    private static native void nativePlay(String mediaId);

    private static native void nativeSearch(String query);

    private static final long ACTIONS = PlaybackState.ACTION_PLAY | PlaybackState.ACTION_PAUSE | PlaybackState.ACTION_PLAY_PAUSE
            | PlaybackState.ACTION_SKIP_TO_NEXT | PlaybackState.ACTION_SKIP_TO_PREVIOUS | PlaybackState.ACTION_SEEK_TO;
    /** What a car can also ask for: a row of the tree, a row of the queue, and something by voice. */
    private static final long CAR_ACTIONS = PlaybackState.ACTION_PLAY_FROM_MEDIA_ID | PlaybackState.ACTION_SKIP_TO_QUEUE_ITEM | PlaybackState.ACTION_PLAY_FROM_SEARCH;

    /** The application's own context, there for as long as the process. */
    private static Context app;
    private static MediaSession session;
    private static int holders;
    /** A car is connected: the session also carries the queue and the car's two extra buttons. */
    private static boolean car;
    private static List<AutoLogic.Row> queue = new ArrayList<>();
    /** What was last said with no player service running, to say it once. */
    private static String idle;

    private PlayerSession() {
    }

    /** The session, made at its first use. Whoever takes it gives it back with {@link #release}. */
    static MediaSession acquire(Context context) {
        if (session == null) {
            app = context.getApplicationContext();
            session = new MediaSession(app, "BitChord");
            session.setCallback(new MediaSession.Callback() {
                @Override
                public void onPlay() {
                    command(AutoLogic.PLAY, 0);
                }

                @Override
                public void onPause() {
                    command(AutoLogic.PAUSE, 0);
                }

                @Override
                public void onSkipToNext() {
                    command(AutoLogic.NEXT, 0);
                }

                @Override
                public void onSkipToPrevious() {
                    command(AutoLogic.PREVIOUS, 0);
                }

                @Override
                public void onSeekTo(long pos) {
                    command(AutoLogic.SEEK, pos);
                }

                @Override
                public void onPlayFromMediaId(String mediaId, Bundle extras) {
                    play(mediaId);
                }

                @Override
                public void onPlayFromSearch(String query, Bundle extras) {
                    if (MainActivity.appRunning()) {
                        search(query);
                    } else {
                        command(AutoLogic.PLAY, 0);
                    }
                }

                @Override
                public void onSkipToQueueItem(long id) {
                    for (AutoLogic.Row row : queue) {
                        if (AutoLogic.queueId(row.id) == id) {
                            play(row.id);
                            return;
                        }
                    }
                }

                @Override
                public void onCustomAction(String action, Bundle extras) {
                    int code = AutoLogic.command(action);
                    if (code != 0) {
                        command(code, 0);
                    }
                }
            });
        }
        holders++;
        return session;
    }

    static void release() {
        if (--holders > 0 || session == null) {
            return;
        }
        session.setActive(false);
        session.release();
        session = null;
        idle = null;
        queue = new ArrayList<>();
    }

    /**
     * A button of the session. Without the app (a process started for a car or a widget
     * alone) there is no player to tell: Play then tries to open the app to play, which
     * Android only lets a background process do in a few cases, so the car keeps saying
     * to open it on the phone.
     */
    private static void command(int code, long arg) {
        if (MainActivity.appRunning()) {
            PlaybackService.command(code, arg);
        } else if (AutoLogic.opensApp(code)) {
            try {
                app.startActivity(new Intent(app, MainActivity.class).setAction(NowPlayingWidget.ACTION_PLAY).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
            } catch (RuntimeException e) {
                // Not from here, then.
            }
        }
    }

    /**
     * "Play … on BitChord": the app searches for the words and plays what it finds; with
     * no words, anything. Only with the app running.
     */
    static void search(String query) {
        nativeSearch(query == null ? "" : query);
    }

    /** A row of the car's tree or queue was tapped. */
    private static void play(String mediaId) {
        if (mediaId != null && MainActivity.appRunning()) {
            nativePlay(mediaId);
        }
    }

    /** A car connected, or the last one left. */
    static void setCar(boolean on) {
        car = on;
        // Without a car there is no queue to carry; either way what plays is said again.
        setQueue(new ArrayList<>());
    }

    /** A row as the session and the car's tree describe it. */
    static MediaDescription describe(AutoLogic.Row row) {
        return new MediaDescription.Builder().setMediaId(row.id).setTitle(row.title).setSubtitle(row.subtitle).build();
    }

    /** What plays next, for the car's queue: the rows of the tree's "Up next". */
    static void setQueue(List<AutoLogic.Row> rows) {
        queue = rows;
        if (session == null) {
            return;
        }
        List<MediaSession.QueueItem> items = new ArrayList<>();
        for (AutoLogic.Row row : rows) {
            long id = AutoLogic.queueId(row.id);
            if (id >= 0) {
                items.add(new MediaSession.QueueItem(describe(row), id));
            }
        }
        session.setQueue(items.isEmpty() ? null : items);
        session.setQueueTitle(items.isEmpty() ? null : "Up next");
        // The song that plays is marked in it.
        PlaybackService.republish();
    }

    private static boolean queued(long id) {
        for (AutoLogic.Row row : queue) {
            if (AutoLogic.queueId(row.id) == id) {
                return true;
            }
        }
        return false;
    }

    /** Tell the session what plays. */
    static void show(AutoLogic.Song song, Bitmap cover) {
        if (session == null) {
            return;
        }
        session.setMetadata(new MediaMetadata.Builder()
                .putString(MediaMetadata.METADATA_KEY_TITLE, song.title)
                .putString(MediaMetadata.METADATA_KEY_ARTIST, song.artist)
                .putLong(MediaMetadata.METADATA_KEY_DURATION, song.duration)
                .putBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART, cover)
                .build());
        PlaybackState.Builder state = new PlaybackState.Builder()
                .setActions(car ? ACTIONS | CAR_ACTIONS : ACTIONS)
                .setState(song.playing ? PlaybackState.STATE_PLAYING : PlaybackState.STATE_PAUSED, song.position, song.playing ? song.speed : 0f);
        if (car) {
            state.addCustomAction(new PlaybackState.CustomAction.Builder(AutoLogic.ACTION_SHUFFLE, "Shuffle up next", R.drawable.ic_auto_shuffle).build());
            int repeat = song.repeat == 1 ? R.drawable.ic_auto_repeat_all : song.repeat == 2 ? R.drawable.ic_auto_repeat_one : R.drawable.ic_auto_repeat_off;
            state.addCustomAction(new PlaybackState.CustomAction.Builder(AutoLogic.ACTION_REPEAT, AutoLogic.repeatLabel(song.repeat), repeat).build());
            if (queued(song.queueIndex)) {
                state.setActiveQueueItemId(song.queueIndex);
            }
        }
        session.setPlaybackState(state.build());
    }

    /**
     * No player service runs (nothing has played yet in this run, or there is no app):
     * the car's browser service says what there is to say. `line`: the app's line of
     * what plays, empty without an app.
     */
    static void idle(int state, String line) {
        if (session == null) {
            return;
        }
        AutoLogic.Song song = AutoLogic.song(line);
        int say = AutoLogic.idle(state, song != null);
        String key = say == AutoLogic.SAY_SONG ? line : String.valueOf(say);
        if (key.equals(idle)) {
            return;
        }
        idle = key;
        if (say == AutoLogic.SAY_SONG) {
            show(song, null);
            return;
        }
        session.setMetadata(null);
        PlaybackState.Builder none = new PlaybackState.Builder();
        if (say == AutoLogic.SAY_OPEN_APP) {
            none.setActions(PlaybackState.ACTION_PLAY | PlaybackState.ACTION_PLAY_PAUSE).setState(PlaybackState.STATE_ERROR, 0, 0f).setErrorMessage(AutoLogic.OPEN_APP);
        } else {
            none.setActions(PlaybackState.ACTION_PLAY_FROM_MEDIA_ID | PlaybackState.ACTION_PLAY_FROM_SEARCH).setState(PlaybackState.STATE_NONE, 0, 0f);
        }
        session.setPlaybackState(none.build());
    }

    /** The player service stopped: what it last said no longer holds. */
    static void playerGone() {
        idle = null;
    }
}
