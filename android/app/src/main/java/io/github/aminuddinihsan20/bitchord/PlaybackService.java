package io.github.aminuddinihsan20.bitchord;

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
import android.media.MediaMetadata;
import android.media.session.MediaSession;
import android.media.session.PlaybackState;
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
 * (polled), and what is pressed goes back to it.
 */
public class PlaybackService extends Service {
    private static final String CHANNEL = "playback";
    private static final int NOTIFICATION_ID = 1;
    private static final String ACTION_PREVIOUS = "io.github.aminuddinihsan20.bitchord.PREVIOUS";
    private static final String ACTION_TOGGLE = "io.github.aminuddinihsan20.bitchord.TOGGLE";
    private static final String ACTION_NEXT = "io.github.aminuddinihsan20.bitchord.NEXT";
    private static final char FIELD = '\u001f';

    // Implemented in android/rust/src/lib.rs.
    private static native String nativeNowPlaying();

    private static native void nativeMediaCommand(int code, long arg);

    private static final int PLAY = 1, PAUSE = 2, TOGGLE = 3, NEXT = 4, PREVIOUS = 5, SEEK = 6;

    private final Handler handler = new Handler(Looper.getMainLooper());
    private MediaSession session;
    private NotificationManager notifications;
    private AudioManager audio;
    private AudioFocusRequest focus;
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
    private Bitmap cover;
    private String coverLoading = "";
    private int emptyPolls;

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
        PowerManager power = getSystemService(PowerManager.class);
        wakeLock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "BitChord:playback");
        wakeLock.setReferenceCounted(false);

        session = new MediaSession(this, "BitChord");
        session.setCallback(new MediaSession.Callback() {
            @Override
            public void onPlay() {
                nativeMediaCommand(PLAY, 0);
            }

            @Override
            public void onPause() {
                nativeMediaCommand(PAUSE, 0);
            }

            @Override
            public void onSkipToNext() {
                nativeMediaCommand(NEXT, 0);
            }

            @Override
            public void onSkipToPrevious() {
                nativeMediaCommand(PREVIOUS, 0);
            }

            @Override
            public void onSeekTo(long pos) {
                nativeMediaCommand(SEEK, pos);
            }
        });
        session.setActive(true);

        // A service started for the foreground must show its notification at once.
        startAsForeground(buildNotification());
        handler.post(poll);
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        String action = intent == null ? null : intent.getAction();
        if (ACTION_PREVIOUS.equals(action)) {
            nativeMediaCommand(PREVIOUS, 0);
        } else if (ACTION_TOGGLE.equals(action)) {
            nativeMediaCommand(TOGGLE, 0);
        } else if (ACTION_NEXT.equals(action)) {
            nativeMediaCommand(NEXT, 0);
        }
        return START_NOT_STICKY;
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
        String[] f = line.split(String.valueOf(FIELD), -1);
        if (f.length < 6) {
            return;
        }
        boolean wasPlaying = playing;
        String newCover = f[2];
        // The seventh field is the playback speed in hundredths.
        float newSpeed = f.length > 6 && parse(f[6]) > 0 ? parse(f[6]) / 100f : 1f;
        // The position moves on by itself while a song plays, and the system counts it
        // on, at the speed it was told, from the last one it was told: only a change it
        // can't work out is published.
        long now = android.os.SystemClock.elapsedRealtime();
        long expected = publishedPosition + (wasPlaying ? (long) ((now - publishedAt) * speed) : 0);
        boolean same = f[0].equals(title) && f[1].equals(artist) && newCover.equals(coverUrl)
                && "1".equals(f[3]) == playing && parse(f[4]) == duration && newSpeed == speed
                && Math.abs(parse(f[5]) - expected) < SEEK_SLACK_MS;
        if (same && published) {
            return;
        }
        title = f[0];
        artist = f[1];
        playing = "1".equals(f[3]);
        duration = parse(f[4]);
        position = parse(f[5]);
        speed = newSpeed;
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

    private static long parse(String s) {
        try {
            return Long.parseLong(s);
        } catch (NumberFormatException e) {
            return 0;
        }
    }

    private void publish() {
        published = true;
        publishedPosition = position;
        publishedAt = android.os.SystemClock.elapsedRealtime();
        session.setMetadata(new MediaMetadata.Builder()
                .putString(MediaMetadata.METADATA_KEY_TITLE, title)
                .putString(MediaMetadata.METADATA_KEY_ARTIST, artist)
                .putLong(MediaMetadata.METADATA_KEY_DURATION, duration)
                .putBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART, cover)
                .build());
        session.setPlaybackState(new PlaybackState.Builder()
                .setActions(PlaybackState.ACTION_PLAY | PlaybackState.ACTION_PAUSE | PlaybackState.ACTION_PLAY_PAUSE
                        | PlaybackState.ACTION_SKIP_TO_NEXT | PlaybackState.ACTION_SKIP_TO_PREVIOUS | PlaybackState.ACTION_SEEK_TO)
                .setState(playing ? PlaybackState.STATE_PLAYING : PlaybackState.STATE_PAUSED, position, playing ? speed : 0f)
                .build());
        notifications.notify(NOTIFICATION_ID, buildNotification());
    }

    private Notification buildNotification() {
        Notification.Builder b = Build.VERSION.SDK_INT >= 26 ? new Notification.Builder(this, CHANNEL) : new Notification.Builder(this);
        b.setSmallIcon(R.drawable.ic_stat_music)
                .setContentTitle(title.isEmpty() ? "BitChord" : title)
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
        session.setActive(false);
        session.release();
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
