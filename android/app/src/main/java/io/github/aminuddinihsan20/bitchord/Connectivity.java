package io.github.aminuddinihsan20.bitchord;

import android.content.Context;
import android.net.ConnectivityManager;
import android.net.Network;
import android.net.NetworkCapabilities;

/**
 * Tells the app what the phone reaches the internet through, when that changes: nothing,
 * a connection that counts the data (mobile data), or one that doesn't (Wi-Fi). Downloads
 * set to "Wi-Fi only" wait for the last.
 */
final class Connectivity {
    private static final int OFFLINE = 0, METERED = 1, UNMETERED = 2;

    // Implemented in android/rust/src/lib.rs.
    private static native void nativeNetwork(int state);

    private static int last = -1;

    private Connectivity() {
    }

    /** Report the connection now, and from here on whenever it changes. */
    static void watch(Context context) {
        ConnectivityManager manager = context.getSystemService(ConnectivityManager.class);
        if (manager == null) {
            // No way to tell: as on a desktop, nothing holds a download back.
            report(UNMETERED);
            return;
        }
        Network active = manager.getActiveNetwork();
        report(state(active == null ? null : manager.getNetworkCapabilities(active)));
        try {
            manager.registerDefaultNetworkCallback(new ConnectivityManager.NetworkCallback() {
                @Override
                public void onCapabilitiesChanged(Network network, NetworkCapabilities capabilities) {
                    report(state(capabilities));
                }

                @Override
                public void onLost(Network network) {
                    report(OFFLINE);
                }
            });
        } catch (RuntimeException e) {
            // Too many callbacks, or the permission was taken away: the state above stays.
        }
    }

    private static int state(NetworkCapabilities capabilities) {
        if (capabilities == null || !capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)) {
            return OFFLINE;
        }
        return capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED) ? UNMETERED : METERED;
    }

    /** Capabilities change often (the signal's strength): the app hears only of a new state. */
    private static synchronized void report(int state) {
        if (state != last) {
            last = state;
            nativeNetwork(state);
        }
    }
}
