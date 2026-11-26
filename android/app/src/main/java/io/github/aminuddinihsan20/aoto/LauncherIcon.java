package io.github.aminuddinihsan20.aoto;

import android.content.ComponentName;
import android.content.Context;
import android.content.pm.PackageManager;

/**
 * The app's icon on the home screen is the one chosen in Settings.
 *
 * Android removes the app's task the moment the entry it was opened through is disabled,
 * which would close the app and stop the music. So a choice is only written down
 * (`wish`), and the entries are switched when the app is closing anyway (`applyWish`).
 */
final class LauncherIcon {
    private static final String PREFS = "launcher_icon";
    private static final String KEY = "wish";

    private LauncherIcon() {}

    /** Remember the icon chosen; it shows once the app has been closed. */
    static boolean wish(Context context, String icon) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString(KEY, IconLogic.shown(icon)).apply();
        return true;
    }

    /** Switch to the icon last chosen, if one was. Only for when the app is on its way out. */
    static void applyWish(Context context) {
        String icon = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY, null);
        if (icon != null) {
            set(context, icon);
        }
    }

    /**
     * Show the icon called `look`. Its entry is enabled before the other is disabled, so there is
     * never a moment with no way into the app; nothing is touched when it already shows.
     */
    private static boolean set(Context context, String look) {
        PackageManager pm = context.getPackageManager();
        String base = MainActivity.class.getPackage().getName();
        try {
            // Two passes: what to enable first, then what to disable.
            for (int pass : new int[] {IconLogic.STATE_ENABLED, IconLogic.STATE_DISABLED}) {
                for (String known : IconLogic.ICONS) {
                    ComponentName entry = new ComponentName(context.getPackageName(), base + IconLogic.alias(known));
                    if (IconLogic.change(known, pm.getComponentEnabledSetting(entry), look) == pass) {
                        pm.setComponentEnabledSetting(entry, pass, PackageManager.DONT_KILL_APP);
                    }
                }
            }
            return true;
        } catch (RuntimeException e) {
            // An entry the system does not know: the icon stays as it is.
            return false;
        }
    }
}
