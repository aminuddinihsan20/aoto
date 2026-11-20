package io.github.aminuddinihsan20.bitchord;

import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.media.browse.MediaBrowser;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.Process;
import android.os.SystemClock;
import android.service.media.MediaBrowserService;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.Iterator;
import java.util.List;

/**
 * What Android Auto connects to: the lists a car's screen browses (what plays next, the
 * downloads, the library's playlists, quick picks) and the media session it shows and
 * controls. The lists are the Rust app's (src/auto.rs), read from it here and never made
 * up: the app runs in the activity's process, so in a process started for the car alone
 * there are none, and the car is told to open BitChord on the phone. Once it is opened
 * (in this same process) the lists appear.
 *
 * The platform's MediaBrowserService, not androidx.media's: it is all Android Auto needs
 * for browsing and playing, and the app gets no new dependency. What only the androidx
 * one has is a search box over the car's lists; asking by voice ("play … on BitChord")
 * goes to the session and works without it.
 */
public class AutoBrowserService extends MediaBrowserService {
    // Implemented in android/rust/src/lib.rs.
    private static native void nativeConnected(boolean on);

    /** Every node of the app's tree with a count of its changes; empty until the app has published it. */
    private static native String nativeVersions();

    private static native String nativeRows(String node);

    /** A load of rows that waits for an app that is starting. */
    private static final class Load {
        final String node;
        final Result<List<MediaBrowser.MediaItem>> result;
        final long asked;

        Load(String node, Result<List<MediaBrowser.MediaItem>> result, long asked) {
            this.node = node;
            this.result = result;
            this.asked = asked;
        }
    }

    private final Handler handler = new Handler(Looper.getMainLooper());
    private final List<Load> waiting = new ArrayList<>();
    /** The app was told a car is here (it can only be told once it runs). */
    private boolean told;
    private String versions = "";

    private final Runnable watch = new Runnable() {
        @Override
        public void run() {
            look();
            handler.postDelayed(this, waiting.isEmpty() ? 500 : 100);
        }
    };

    @Override
    public void onCreate() {
        super.onCreate();
        setSessionToken(PlayerSession.acquire(this).getSessionToken());
        PlayerSession.setCar(true);
        handler.post(watch);
    }

    @Override
    public void onDestroy() {
        handler.removeCallbacks(watch);
        if (told) {
            nativeConnected(false);
        }
        PlayerSession.setCar(false);
        PlayerSession.release();
        super.onDestroy();
    }

    @Override
    public BrowserRoot onGetRoot(String clientPackageName, int clientUid, Bundle rootHints) {
        PackageManager packages = getPackageManager();
        String[] owned = packages.getPackagesForUid(clientUid);
        boolean packageIsCallers = clientPackageName != null && owned != null && Arrays.asList(owned).contains(clientPackageName);
        boolean systemApp = false;
        if (packageIsCallers) {
            try {
                systemApp = (packages.getApplicationInfo(clientPackageName, 0).flags & (ApplicationInfo.FLAG_SYSTEM | ApplicationInfo.FLAG_UPDATED_SYSTEM_APP)) != 0;
            } catch (PackageManager.NameNotFoundException e) {
                // Not one the app can see: not a system app, then.
            }
        }
        boolean debuggable = (getApplicationInfo().flags & ApplicationInfo.FLAG_DEBUGGABLE) != 0;
        boolean recentOnly = rootHints != null && rootHints.getBoolean(BrowserRoot.EXTRA_RECENT);
        if (!AutoLogic.serves(recentOnly) || !AutoLogic.allowed(clientUid == Process.myUid(), clientUid == Process.SYSTEM_UID, packageIsCallers, systemApp, clientPackageName, debuggable)) {
            return null;
        }
        return new BrowserRoot(AutoLogic.ROOT, null);
    }

    @Override
    public void onLoadChildren(String parentId, Result<List<MediaBrowser.MediaItem>> result) {
        int load = AutoLogic.load(AutoLogic.state(MainActivity.appRunning(), published()), 0);
        if (load == AutoLogic.WAIT) {
            // The app is starting: its lists come in a moment (look() answers).
            result.detach();
            waiting.add(new Load(parentId, result, SystemClock.elapsedRealtime()));
            handler.removeCallbacks(watch);
            handler.postDelayed(watch, 100);
        } else {
            result.sendResult(load == AutoLogic.SEND ? items(parentId) : new ArrayList<>());
        }
    }

    /** What the app has published of its tree: nothing without an app. The app is told a car is here as soon as it runs. */
    private String published() {
        if (!MainActivity.appRunning()) {
            return "";
        }
        if (!told) {
            told = true;
            nativeConnected(true);
        }
        return nativeVersions();
    }

    private List<MediaBrowser.MediaItem> items(String node) {
        List<MediaBrowser.MediaItem> items = new ArrayList<>();
        for (AutoLogic.Row row : AutoLogic.rows(nativeRows(node))) {
            items.add(new MediaBrowser.MediaItem(PlayerSession.describe(row), row.playable ? MediaBrowser.MediaItem.FLAG_PLAYABLE : MediaBrowser.MediaItem.FLAG_BROWSABLE));
        }
        return items;
    }

    /**
     * Twice a second while a car is connected: answer the loads that waited, tell the
     * car which lists changed, keep the session's queue, and, while no player service
     * runs, say what the session has to say (a paused song, nothing, or "open the app").
     */
    private void look() {
        boolean app = MainActivity.appRunning();
        String latest = published();
        int state = AutoLogic.state(app, latest);
        long now = SystemClock.elapsedRealtime();
        for (Iterator<Load> each = waiting.iterator(); each.hasNext();) {
            Load load = each.next();
            int todo = AutoLogic.load(state, now - load.asked);
            if (todo != AutoLogic.WAIT) {
                each.remove();
                load.result.sendResult(todo == AutoLogic.SEND ? items(load.node) : new ArrayList<>());
            }
        }
        for (String node : AutoLogic.changed(versions, latest)) {
            if (AutoLogic.QUEUE.equals(node)) {
                PlayerSession.setQueue(app ? AutoLogic.rows(nativeRows(node)) : new ArrayList<>());
            }
            notifyChildrenChanged(node);
        }
        versions = latest;
        if (!PlaybackService.running()) {
            PlayerSession.idle(state, app ? PlaybackService.nowPlaying() : "");
        }
    }
}
