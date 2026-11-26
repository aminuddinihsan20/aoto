package io.github.aminuddinihsan20.aoto;

import android.content.ComponentName;
import android.content.Context;
import android.content.pm.PackageManager;

/** The app's icon on the home screen is the one chosen in Settings. */
final class LauncherIcon {
    private LauncherIcon() {}

    /**
     * Show the icon called `look`. Its entry is enabled before the other is disabled, so there is
     * never a moment with no way into the app; nothing is touched when it already shows.
     */
    static boolean set(Context context, String look) {
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
