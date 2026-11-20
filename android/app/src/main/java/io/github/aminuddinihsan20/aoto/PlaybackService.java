package io.github.aminuddinihsan20.aoto;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.pm.ServiceInfo;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.drawable.Icon;
import android.media.AudioAttributes;
import android.media.AudioFocusRequest;
import android.media.AudioManager;
import android.media.session.MediaSession;
import android.os.Build;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.os.PowerManager;

import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;

/**
 * Keeps the music going with the app away, and shows it: a foreground service with a
 * media notification (title, artist, artwork, previous / play-pause / next) and a media
 * session for the lock screen and headset buttons. What plays comes from the Rust app
 * (polled), and what is pressed goes back to it. The home-screen widgets (NowPlayingWidget)
 * show the same and are told from here whenever it changes. The media session itself is
 * the process's (PlayerSession), shared with a car's browser service.
 */
public class PlaybackService extends Service {
    private static final String CHANNEL = "playback";
    private static final int NOTIFICATION_ID = 1;
    private static final String ACTION_PREVIOUS = "io.github.aminuddinihsan20.aoto.PREVIOUS";
    private static final String ACTION_TOGGLE = "io.github.aminuddinihsan20.aoto.TOGGLE";
    private static final String ACTION_NEXT = "io.github.aminuddinihsan20.aoto.NEXT";

    // Implemented in android/rust/src/lib.rs.
    private static native String nativeNowPlaying();

    private static native void nativeMediaCommand(int code, long arg);

    private static final int PLAY = AutoLogic.PLAY, PAUSE = AutoLogic.PAUSE, TOGGLE = AutoLogic.TOGGLE, NEXT = AutoLogic.NEXT, PREVIOUS = AutoLogic.PREVIOUS;

    /** The service that runs, if one does (main thread only). */
    private static PlaybackService running;

    private final Handler handler = new Handler(Looper.getMainLooper());
    private MediaSession session;
    private NotificationManager notifications;
    private AudioManager audio;
    private AudioFocusRequest focus;
    private PowerManager power;
    private PowerManager.WakeLock wakeLock;
    private boolean pausedForFocus;
    private boolean focusHeld;
    private boolean noisyRegistered;

    private String lastLine = "";
    /** How far the position may drift from the system's own count before it is told again. */
    private static final long SEEK_SLACK_MS = 1500;
    private boolean published;
    private long publishedPosition;
    private long publishedAt;
    private String title = "", artist = "", coverUrl = "";
    private boolean playing;
    private long duration, position;
    /** How fast the position moves while playing: 1 as recorded (the app's playback speed). */
    private float speed = 1f;
    /** How the queue repeats and the song's place in it, for a car's buttons and queue. */
    private int repeat;
    private long queueIndex = -1;
    private Bitmap cover;
    private String coverLoading = "";
    private int emptyPolls;
    /** The percent the widgets' progress bar shows, and when it was last moved. */
    private int widgetPercent;
    private long widgetProgressAt;

    private final Runnable poll = new Runnable() {
        @Override
        public void run() {
            refresh();
            handler.postDelayed(this, 500);
        }
    };

    private final BroadcastReceiver noisy = new BroadcastReceiver() {
        @Override
        public void onReceive(Context context, Intent intent) {
            // Headphones unplugged: stop, as every music app does.
            nativeMediaCommand(PAUSE, 0);
        }
    };

    @Override
    public void onCreate() {
        super.onCreate();
        notifications = getSystemService(NotificationManager.class);
        audio = getSystemService(AudioManager.class);
        if (Build.VERSION.SDK_INT >= 26) {
            notifications.createNotificationChannel(new NotificationChannel(CHANNEL, "Playback", NotificationManager.IMPORTANCE_LOW));
        }
        power = getSystemService(PowerManager.class);
        wakeLock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "Aoto:playback");
        wakeLock.setReferenceCounted(false);

        session = PlayerSession.acquire(this);
        running = this;
        // Without the app (see onStartCommand) there is nothing to ask and nothing to tell.
        boolean app = MainActivity.appRunning();
        session.setActive(app);

        // A service started for the foreground must show its notification at once.
        startAsForeground(buildNotification());
        if (app) {
            handler.post(poll);
        }
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        String action = intent == null ? null : intent.getAction();
        if (!MainActivity.appRunning()) {
            leftBehind(action);
            return START_NOT_STICKY;
        }
        if (ACTION_PREVIOUS.equals(action)) {
            nativeMediaCommand(PREVIOUS, 0);
        } else if (ACTION_TOGGLE.equals(action)) {
            nativeMediaCommand(TOGGLE, 0);
        } else if (ACTION_NEXT.equals(action)) {
            nativeMediaCommand(NEXT, 0);
        }
        return START_NOT_STICKY;
    }

    /** What a widget's button sends here: the same as the notification's. */
    static Intent widgetButton(Context context, int control) {
        String action = control == WidgetLogic.PREVIOUS ? ACTION_PREVIOUS : control == WidgetLogic.NEXT ? ACTION_NEXT : ACTION_TOGGLE;
        return new Intent(context, PlaybackService.class).setAction(action);
    }

    /** Play, as the app's own button: what MainActivity does when a widget opened it to play. */
    static void play() {
        nativeMediaCommand(PLAY, 0);
    }

    /** A button of the media session (PlayerSession), for the app. Only with the app running. */
    static void command(int code, long arg) {
        nativeMediaCommand(code, arg);
    }

    /** The app's line of what plays. Only with the app running. */
    static String nowPlaying() {
        return nativeNowPlaying();
    }

    /** Does the player service run? Then it is the one that tells the session what plays. */
    static boolean running() {
        return running != null;
    }

    /** What the session carries changed (a car came or went, its queue arrived): say what plays again, as it is now. */
    static void republish() {
        if (running != null && MainActivity.appRunning()) {
            running.lastLine = "";
            running.published = false;
            running.refresh();
        }
    }

    /**
     * Started in a process without the app: by a button of a widget that still showed a
     * song from a run that was killed with no chance to say so. Music is played by the
     * app, which only runs in its activity, so there is no player to tell: the app is
     * opened (to play, if the button was Play) and the service goes again, which sets the
     * widgets right (onDestroy). Should Android not let the app open from here, they now
     * say "Play something", and that opens it.
     */
    private void leftBehind(String action) {
        int control = ACTION_TOGGLE.equals(action) ? WidgetLogic.TOGGLE
                : ACTION_PREVIOUS.equals(action) ? WidgetLogic.PREVIOUS
                : ACTION_NEXT.equals(action) ? WidgetLogic.NEXT : WidgetLogic.BODY;
        Intent open = new Intent(this, MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        if (WidgetLogic.coldTap(control) == WidgetLogic.OPEN_AND_PLAY) {
            open.setAction(NowPlayingWidget.ACTION_PLAY);
        }
        try {
            startActivity(open);
        } catch (RuntimeException e) {
            // Not from here, then.
        }
        stopForeground(STOP_FOREGROUND_REMOVE);
        stopSelf();
    }

    private void startAsForeground(Notification n) {
        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(NOTIFICATION_ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK);
        } else {
            startForeground(NOTIFICATION_ID, n);
        }
    }

    /** Read what the app says plays and bring the session and notification up to date. */
    private void refresh() {
        String line = nativeNowPlaying();
        if (line == null || line.isEmpty()) {
            // Nothing is loaded any more: give it a few seconds, then go.
            if (++emptyPolls > 6) {
                stopSelf();
            }
            return;
        }
        emptyPolls = 0;
        if (line.equals(lastLine)) {
            return;
        }
        lastLine = line;
        AutoLogic.Song now = AutoLogic.song(line);
        if (now == null) {
            return;
        }
        boolean wasPlaying = playing;
        String newCover = now.cover;
        // The position moves on by itself while a song plays, and the system counts it
        // on, at the speed it was told, from the last one it was told: only a change it
        // can't work out is published.
        long clock = android.os.SystemClock.elapsedRealtime();
        long expected = publishedPosition + (wasPlaying ? (long) ((clock - publishedAt) * speed) : 0);
        boolean same = now.title.equals(title) && now.artist.equals(artist) && newCover.equals(coverUrl)
                && now.playing == playing && now.duration == duration && now.speed == speed
                && now.repeat == repeat && now.queueIndex == queueIndex
                && Math.abs(now.position - expected) < SEEK_SLACK_MS;
        if (same && published) {
            if (playing) {
                moveWidgetProgress(now.position);
            }
            return;
        }
        title = now.title;
        artist = now.artist;
        playing = now.playing;
        duration = now.duration;
        position = now.position;
        speed = now.speed;
        repeat = now.repeat;
        queueIndex = now.queueIndex;
        if (!newCover.equals(coverUrl)) {
            coverUrl = newCover;
            cover = null;
            loadCover(newCover);
        }
        if (playing && !wasPlaying) {
            requestFocus();
            if (!wakeLock.isHeld()) {
                wakeLock.acquire();
            }
            if (!noisyRegistered) {
                registerReceiver(noisy, new IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY));
                noisyRegistered = true;
            }
        } else if (!playing && wasPlaying && wakeLock.isHeld() && !pausedForFocus) {
            wakeLock.release();
        }
        publish();
    }

    private void publish() {
        published = true;
        publishedPosition = position;
        publishedAt = android.os.SystemClock.elapsedRealtime();
        AutoLogic.Song song = new AutoLogic.Song();
        song.title = title;
        song.artist = artist;
        song.playing = playing;
        song.duration = duration;
        song.position = position;
        song.speed = speed;
        song.repeat = repeat;
        song.queueIndex = queueIndex;
        PlayerSession.show(song, cover);
        notifications.notify(NOTIFICATION_ID, buildNotification());
        widgetPercent = WidgetLogic.percent(position, duration);
        widgetProgressAt = publishedAt;
        NowPlayingWidget.show(this, title, artist, playing, widgetPercent, cover);
    }

    /**
     * A widget's progress bar can't count by itself as the notification's does: while a
     * song plays it is told a percent at a time (WidgetLogic.progressDue), from the poll
     * that runs anyway, and not at all with the screen off.
     */
    private void moveWidgetProgress(long positionNow) {
        int percent = WidgetLogic.percent(positionNow, duration);
        long now = android.os.SystemClock.elapsedRealtime();
        if (WidgetLogic.progressDue(widgetPercent, percent, now - widgetProgressAt) && power.isInteractive()) {
            widgetPercent = percent;
            widgetProgressAt = now;
            NowPlayingWidget.progress(this, percent);
        }
    }

    private Notification buildNotification() {
        Notification.Builder b = Build.VERSION.SDK_INT >= 26 ? new Notification.Builder(this, CHANNEL) : new Notification.Builder(this);
        b.setSmallIcon(R.drawable.ic_stat_music)
                .setContentTitle(title.isEmpty() ? "Aoto" : title)
                .setContentText(artist)
                .setLargeIcon(cover)
                .setOngoing(playing)
                .setOnlyAlertOnce(true)
                .setShowWhen(false)
                .setVisibility(Notification.VISIBILITY_PUBLIC)
                .setContentIntent(PendingIntent.getActivity(this, 0, new Intent(this, MainActivity.class), PendingIntent.FLAG_IMMUTABLE))
                .addAction(action(android.R.drawable.ic_media_previous, "Previous", ACTION_PREVIOUS))
                .addAction(action(playing ? android.R.drawable.ic_media_pause : android.R.drawable.ic_media_play, playing ? "Pause" : "Play", ACTION_TOGGLE))
                .addAction(action(android.R.drawable.ic_media_next, "Next", ACTION_NEXT))
                .setStyle(new Notification.MediaStyle().setMediaSession(session.getSessionToken()).setShowActionsInCompactView(0, 1, 2));
        return b.build();
    }

    private Notification.Action action(int icon, String label, String name) {
        Intent intent = new Intent(this, PlaybackService.class).setAction(name);
        PendingIntent pending = PendingIntent.getService(this, name.hashCode(), intent, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        return new Notification.Action.Builder(Icon.createWithResource(this, icon), label, pending).build();
    }

    private void loadCover(final String url) {
        if (url.isEmpty() || url.equals(coverLoading)) {
            return;
        }
        coverLoading = url;
        new Thread(() -> {
            Bitmap bitmap = null;
            try {
                HttpURLConnection c = (HttpURLConnection) new URL(url).openConnection();
                c.setConnectTimeout(8000);
                c.setReadTimeout(8000);
                try (InputStream in = c.getInputStream()) {
                    bitmap = BitmapFactory.decodeStream(in);
                }
            } catch (Exception ignored) {
                // No artwork: the notification just has none.
            }
            final Bitmap result = bitmap;
            handler.post(() -> {
                if (url.equals(coverUrl) && result != null) {
                    cover = result;
                    publish();
                }
            });
        }).start();
    }

    /**
     * Asks for audio focus once and keeps the one request: a second request from here, with
     * a listener of its own, makes the system tell the first one it lost focus.
     */
    private void requestFocus() {
        if (focus == null) {
            AudioManager.OnAudioFocusChangeListener listener = change -> {
                if (change == AudioManager.AUDIOFOCUS_LOSS) {
                    pausedForFocus = false;
                    nativeMediaCommand(PAUSE, 0);
                } else if (change == AudioManager.AUDIOFOCUS_LOSS_TRANSIENT) {
                    pausedForFocus = true;
                    nativeMediaCommand(PAUSE, 0);
                } else if (change == AudioManager.AUDIOFOCUS_GAIN && pausedForFocus) {
                    pausedForFocus = false;
                    nativeMediaCommand(PLAY, 0);
                }
            };
            focus = new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
                    .setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build())
                    .setOnAudioFocusChangeListener(listener)
                    .build();
        }
        if (!focusHeld) {
            focusHeld = audio.requestAudioFocus(focus) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED;
        }
    }

    @Override
    public void onDestroy() {
        handler.removeCallbacks(poll);
        if (noisyRegistered) {
            unregisterReceiver(noisy);
        }
        if (focus != null) {
            audio.abandonAudioFocusRequest(focus);
        }
        if (wakeLock.isHeld()) {
            wakeLock.release();
        }
        running = null;
        session.setActive(false);
        // A car's browser service may still hold the session: it says what there is to say from here.
        PlayerSession.playerGone();
        PlayerSession.release();
        // No player any more: the widgets' buttons would have no one to tell.
        NowPlayingWidget.clear(this);
        super.onDestroy();
    }

    /** The app was swiped out of the recent apps: the music stops with it. */
    @Override
    public void onTaskRemoved(Intent rootIntent) {
        stopForeground(STOP_FOREGROUND_REMOVE);
        stopSelf();
        MainActivity.endProcess(this);
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
