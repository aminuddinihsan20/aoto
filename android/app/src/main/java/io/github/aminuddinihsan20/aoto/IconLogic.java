package io.github.aminuddinihsan20.aoto;

/**
 * Which of the app's home-screen entries shows for the icon chosen, with nothing of Android in it
 * (so it is checked with a JDK alone: `sh android/test-java.sh`).
 */
final class IconLogic {
    /** The icons, each an entry of the manifest; the first is the one enabled there. */
    static final String[] ICONS = {"indigo", "sticker", "sky", "rose", "midnight"};

    /** PackageManager's states of a component. */
    static final int STATE_DEFAULT = 0;
    static final int STATE_ENABLED = 1;
    static final int STATE_DISABLED = 2;

    private IconLogic() {}

    /** The look whose entry shows for `look`: itself, or the first for one that has none. */
    static String shown(String look) {
        for (String known : ICONS) {
            if (known.equals(look)) {
                return known;
            }
        }
        return ICONS[0];
    }

    /** The entry's class name under the app's package, as the manifest names it. */
    static String alias(String look) {
        String shown = shown(look);
        return ".Launcher" + Character.toUpperCase(shown.charAt(0)) + shown.substring(1);
    }

    /** Is a look's entry on the home screen, from its state? Left alone, only the first is. */
    static boolean enabled(String look, int state) {
        return state == STATE_ENABLED || (state == STATE_DEFAULT && ICONS[0].equals(look));
    }

    /**
     * What to do to `look`'s entry so that only `wanted`'s shows: STATE_ENABLED,
     * STATE_DISABLED, or -1 to leave it (it is already so: telling Android again would
     * make the launcher redraw for nothing).
     */
    static int change(String look, int state, String wanted) {
        boolean want = look.equals(shown(wanted));
        if (enabled(look, state) == want) {
            return -1;
        }
        return want ? STATE_ENABLED : STATE_DISABLED;
    }
}
