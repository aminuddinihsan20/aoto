package io.github.aminuddinihsan20.bitchord;

import android.Manifest;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.provider.DocumentsContract;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.view.KeyEvent;
import android.view.View;

import androidx.activity.OnBackPressedCallback;
import androidx.core.graphics.Insets;
import androidx.core.view.ViewCompat;
import androidx.core.view.WindowInsetsCompat;

import com.google.androidgamesdk.GameActivity;

/**
 * Loads libbitchord_android.so (named in the manifest), whose android_main runs the app,
 * and tells it what only Java can: the system bars' size and the back button.
 */
public class MainActivity extends GameActivity {
    // Implemented in android/rust/src/lib.rs.
    private static native void nativeInsets(int left, int top, int right, int bottom);

    private static native boolean nativeCanGoBack();

    private static native void nativeBack();

    private static native void nativeEnter();

    /** A sign-in the app asked for: 1 Google, 2 Spotify, 0 none. */
    private static native int nativeTakeLoginRequest();

    /** The app is away (stopped) or back: the music is kept going from a thread while away. */
    private static native void nativeSetBackground(boolean away);

    /** What plays, for the media notification; empty when nothing is loaded. */
    private static native String nativeNowPlaying();

    private boolean hadMedia;

    private static volatile boolean appRunning;

    /**
     * Whether the Rust app runs in this process. It starts with this activity and the
     * process ends with it, so a process without it (started for a widget alone) has no
     * player, and not even the native library loaded.
     */
    static boolean appRunning() {
        return appRunning;
    }

    /** The next job the app asks of Java ("id kind arg" joined by U+001F), or empty. */
    private static native String nativeTakeCall();

    /** The answer to the job with this id. */
    private static native void nativeAnswer(long id, boolean ok, String text);

    private static final int REQUEST_FOLDER = 7;
    private long folderCall;

    /** Asking for the storage permission, which saving a picture needs before Android 10. */
    private static final int REQUEST_STORAGE = 8;
    /** The save waiting for that answer: its call, the picture's file and its name. */
    private long saveCall;
    private String[] saveArgs;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        appRunning = true;
        // Whatever an earlier run left on the home screen: nothing plays yet.
        NowPlayingWidget.refresh(this);
        if (savedInstanceState == null) {
            playIfAsked(getIntent());
        }

        // Edge to edge: the app draws under the status and gesture bars and keeps its
        // content clear of them (and above the keyboard) by these insets.
        View decor = getWindow().getDecorView();
        ViewCompat.setOnApplyWindowInsetsListener(decor, (view, insets) -> {
            Insets bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() | WindowInsetsCompat.Type.displayCutout());
            Insets ime = insets.getInsets(WindowInsetsCompat.Type.ime());
            nativeInsets(bars.left, bars.top, bars.right, Math.max(bars.bottom, ime.bottom));
            return insets;
        });
        ViewCompat.requestApplyInsets(decor);

        // The app asks for a sign-in by setting a request; open the sign-in page for it.
        Handler main = new Handler(Looper.getMainLooper());
        // Downloads wait for Wi-Fi when asked to: the app hears what the phone is on.
        main.post(() -> Connectivity.watch(getApplicationContext()));
        main.post(new Runnable() {
            @Override
            public void run() {
                String now = nativeNowPlaying();
                boolean loaded = now != null && !now.isEmpty();
                // Fields are joined by U+001F; the fourth says whether it plays.
                String[] fields = loaded ? now.split("\u001f", -1) : new String[0];
                boolean playing = fields.length > 3 && "1".equals(fields[3]);
                // The player (notification and all) starts with the first song that plays,
                // not with last time's song restored paused, and stays while one is loaded.
                // Tried again if Android refused it.
                hadMedia = loaded && (hadMedia || (playing && startPlaybackService()));
                String call = nativeTakeCall();
                if (call != null && !call.isEmpty()) {
                    handleCall(call);
                }
                int site = nativeTakeLoginRequest();
                if (site != 0) {
                    startActivity(new Intent(MainActivity.this, LoginActivity.class).putExtra(LoginActivity.EXTRA_SITE, site));
                }
                main.postDelayed(this, 250);
            }
        });

        // Back closes what the app has open (Now Playing, then a page); with nothing to
        // close the app goes to the background and keeps playing, as music apps do.
        getOnBackPressedDispatcher().addCallback(this, new OnBackPressedCallback(true) {
            @Override
            public void handleOnBackPressed() {
                if (nativeCanGoBack()) {
                    nativeBack();
                } else {
                    moveTaskToBack(true);
                }
            }
        });
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        playIfAsked(intent);
    }

    /**
     * A widget's Play with no player to tell opens the app to play: as Play in the app,
     * which starts last time's song. (Not when Android reopens the app from the recent
     * apps with the intent it was first started with.)
     */
    private static void playIfAsked(Intent intent) {
        if (intent != null && NowPlayingWidget.ACTION_PLAY.equals(intent.getAction())
                && (intent.getFlags() & Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY) == 0) {
            PlaybackService.play();
        }
    }

    /** A job the app asked for: choosing a folder, listing music, copying files. */
    private void handleCall(String line) {
        String[] f = line.split("\u001f", 3);
        if (f.length < 3) {
            return;
        }
        final long id = Long.parseLong(f[0]);
        String kind = f[1], arg = f[2];
        Storage.Done done = (ok, text) -> nativeAnswer(id, ok, text);
        switch (kind) {
            case "pick-folder": {
                Intent pick = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
                        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                                | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION | Intent.FLAG_GRANT_PREFIX_URI_PERMISSION);
                folderCall = id;
                startActivityForResult(pick, REQUEST_FOLDER);
                break;
            }
            case "turnstile":
                Turnstile.run(this, done);
                break;
            case "open-url":
                // Web pages only: nothing the app hands over may start another kind of app.
                if (!arg.startsWith("https://") && !arg.startsWith("http://")) {
                    done.finish(false, "not a web address");
                    break;
                }
                try {
                    startActivity(new Intent(Intent.ACTION_VIEW, android.net.Uri.parse(arg)));
                    done.finish(true, "");
                } catch (RuntimeException e) {
                    done.finish(false, "no browser to open it with");
                }
                break;
            case "scan":
                Storage.scan(this, arg, done);
                break;
            case "fetch":
                Storage.fetch(this, arg, done);
                break;
            case "export": {
                String[] e = arg.split("\u001e", 4);
                if (e.length < 4) {
                    done.finish(false, "bad export request");
                } else {
                    Storage.export(this, e[0], e[1], e[2], e[3], done);
                }
                break;
            }
            case "lyric-card":
                LyricCard.render(this, arg, done);
                break;
            case "save-image": {
                String[] s = arg.split("\u001e", 2);
                if (s.length < 2) {
                    done.finish(false, "bad save request");
                } else if (Build.VERSION.SDK_INT < 29 && checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) != PackageManager.PERMISSION_GRANTED) {
                    // Before Android 10 the Pictures folder is written to directly: ask first.
                    saveCall = id;
                    saveArgs = s;
                    requestPermissions(new String[] {Manifest.permission.WRITE_EXTERNAL_STORAGE}, REQUEST_STORAGE);
                } else {
                    LyricCard.save(this, s[0], s[1], done);
                }
                break;
            }
            case "share-image":
                LyricCard.share(this, arg, done);
                break;
            default:
                done.finish(false, "unknown request " + kind);
        }
    }

    @Override
    public void onRequestPermissionsResult(int requestCode, String[] permissions, int[] results) {
        super.onRequestPermissionsResult(requestCode, permissions, results);
        if (requestCode != REQUEST_STORAGE || saveCall == 0) {
            return;
        }
        final long id = saveCall;
        String[] s = saveArgs;
        saveCall = 0;
        saveArgs = null;
        if (results.length > 0 && results[0] == PackageManager.PERMISSION_GRANTED) {
            LyricCard.save(this, s[0], s[1], (ok, text) -> nativeAnswer(id, ok, text));
        } else {
            nativeAnswer(id, false, "the storage permission was not given");
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != REQUEST_FOLDER || folderCall == 0) {
            return;
        }
        long id = folderCall;
        folderCall = 0;
        Uri uri = resultCode == RESULT_OK && data != null ? data.getData() : null;
        if (uri == null) {
            nativeAnswer(id, false, "cancelled");
            return;
        }
        try {
            getContentResolver().takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
        } catch (SecurityException e) {
            // Read-only is enough for music.
            try {
                getContentResolver().takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION);
            } catch (SecurityException ignored) {
                nativeAnswer(id, false, "no permission for that folder");
                return;
            }
        }
        nativeAnswer(id, true, uri + "\u001f" + DocumentsContract.getTreeDocumentId(uri));
    }

    @Override
    protected void onStart() {
        super.onStart();
        nativeSetBackground(false);
    }

    @Override
    protected void onStop() {
        nativeSetBackground(true);
        super.onStop();
    }

    /**
     * The activity is going away for good (swiped out of the recent apps, or finished). The
     * Rust app can't outlive it: GameActivity's onDestroy waits for the app's thread to
     * return, which it never does, so the main thread would hang ("isn't responding" at the
     * next launch) with the music service keeping the process alive. End the music and the
     * process instead; the next launch starts fresh.
     */
    @Override
    protected void onDestroy() {
        endProcess(this);
    }

    /** Stop the music service and end the process. Never returns. */
    static void endProcess(android.content.Context context) {
        // The widgets outlive the process: they must not be left showing a song and
        // buttons for a player that is gone.
        NowPlayingWidget.clearNow(context);
        try {
            context.stopService(new Intent(context, PlaybackService.class));
        } catch (RuntimeException ignored) {
            // Already gone.
        }
        // The app saves its settings when the activity stops, just before this: let it finish.
        try {
            Thread.sleep(250);
        } catch (InterruptedException ignored) {
            // Ending anyway.
        }
        android.os.Process.killProcess(android.os.Process.myPid());
        System.exit(0);
    }

    /** The first song: ask once for notifications (Android 13+), then show the player. */
    private boolean startPlaybackService() {
        if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(new String[] {Manifest.permission.POST_NOTIFICATIONS}, 1);
        }
        try {
            startForegroundService(new Intent(this, PlaybackService.class));
            return true;
        } catch (IllegalStateException e) {
            // Android doesn't let a service start from the background: it is tried again
            // at the next poll, and works once the app is on screen.
            return false;
        }
    }

    // winit reports Enter as the character '\n', which the app's text fields ignore: hand
    // it over as the Enter key (the keyboard's action key sends it too).
    @Override
    public boolean dispatchKeyEvent(KeyEvent event) {
        if (event.getKeyCode() == KeyEvent.KEYCODE_ENTER) {
            if (event.getAction() == KeyEvent.ACTION_DOWN && event.getRepeatCount() == 0) {
                nativeEnter();
            }
            return true;
        }
        return super.dispatchKeyEvent(event);
    }
}
