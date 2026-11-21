package io.github.aminuddinihsan20.aoto;

/**
 * The decisions of the update check, with no Android in them: is a release newer, does it
 * have this app's APK, and has the listener been told of it already. The check itself
 * (the network, the notification) is in {@link UpdateCheckJob}.
 */
final class UpdateLogic {
    private UpdateLogic() {}

    /** `a` is newer than `b`: dotted numbers, a leading "v" ignored, a missing part is 0. */
    static boolean newer(String a, String b) {
        int[] x = parts(a);
        int[] y = parts(b);
        for (int i = 0; i < Math.max(x.length, y.length); i++) {
            int p = i < x.length ? x[i] : 0;
            int q = i < y.length ? y[i] : 0;
            if (p != q) {
                return p > q;
            }
        }
        return false;
    }

    private static int[] parts(String version) {
        String v = version == null ? "" : version.trim();
        if (v.startsWith("v") || v.startsWith("V")) {
            v = v.substring(1);
        }
        String[] pieces = v.split("\\.");
        int[] out = new int[pieces.length];
        for (int i = 0; i < pieces.length; i++) {
            try {
                out[i] = Integer.parseInt(pieces[i]);
            } catch (NumberFormatException e) {
                out[i] = 0;
            }
        }
        return out;
    }

    /**
     * What the latest release offers this phone: {version, url} of its APK, when `tag` is
     * newer than `current` and one of the assets is that version's APK (named the way
     * release.yml names it), downloaded over HTTPS. Otherwise null.
     */
    static String[] offer(String tag, String current, String[] names, String[] urls) {
        if (tag == null || !newer(tag, current)) {
            return null;
        }
        String version = tag.startsWith("v") ? tag.substring(1) : tag;
        String want = "aoto-" + version + "-android-arm64.apk";
        for (int i = 0; i < names.length && i < urls.length; i++) {
            if (want.equals(names[i])) {
                String url = urls[i];
                if (url != null && url.startsWith("https://")) {
                    return new String[] {version, url};
                }
                return null;
            }
        }
        return null;
    }

    /** Tell the listener of `version`, unless they were told of it (or a newer one) already. */
    static boolean shouldAnnounce(String version, String lastAnnounced) {
        return lastAnnounced == null || lastAnnounced.isEmpty() || newer(version, lastAnnounced);
    }

    static String title(String version) {
        return "Aoto " + version + " is available";
    }

    static String text() {
        return "Tap to download. It installs over this version: nothing to uninstall, your music and settings stay.";
    }
}
