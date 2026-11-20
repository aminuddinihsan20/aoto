package io.github.aminuddinihsan20.bitchord;

import android.app.Activity;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.media.AudioAttributes;
import android.media.AudioDeviceCallback;
import android.media.AudioDeviceInfo;
import android.media.AudioManager;
import android.media.MediaRouter;
import android.media.MediaRouter2;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.provider.Settings;

import java.util.List;

/**
 * "Play on": tells the app where Android sends the music (the phone's speaker, a
 * Bluetooth or wired device) and the media volume, whenever either changes, and opens
 * Android's output switcher for it. Android does the switching; the app only shows it.
 *
 * No permission is needed: a device's name comes with the audio device
 * (AudioDeviceInfo.getProductName), not from Bluetooth, and a device that gives none is
 * shown by its kind.
 */
final class AudioOutput {
    // Implemented in android/rust/src/lib.rs.
    private static native void nativeOutput(String line);

    /** What Android's volume keys and panel broadcast; not a public constant, but the one Media3 listens to as well. */
    private static final String VOLUME_CHANGED = "android.media.VOLUME_CHANGED_ACTION";
    /** The output switcher on Android 12 and 13: a broadcast to the System UI. */
    private static final String SYSTEM_UI = "com.android.systemui";
    private static final String SYSTEM_UI_SWITCHER = "com.android.systemui.action.LAUNCH_MEDIA_OUTPUT_DIALOG";
    private static final String SYSTEM_UI_PACKAGE = "package_name";
    /** The output switcher on Android 10 and 11: a panel of the Settings app. */
    private static final String SETTINGS_PANEL = "com.android.settings.panel.action.MEDIA_OUTPUT";
    private static final String SETTINGS_PANEL_PACKAGE = "com.android.settings.panel.extra.PACKAGE_NAME";

    private static final AudioAttributes MUSIC =
            new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build();

    private static AudioManager audio;
    private static int offer;
    private static String last = "";

    private AudioOutput() {
    }

    /** Report the output and the volume now, and from here on whenever they change. Called once, on the main thread. */
    static void watch(Context context) {
        if (audio != null) {
            return;
        }
        Context app = context.getApplicationContext();
        audio = app.getSystemService(AudioManager.class);
        if (audio == null) {
            return;
        }
        offer = offer(app);
        Handler main = new Handler(Looper.getMainLooper());
        // A device plugged in, connected or gone.
        audio.registerAudioDeviceCallback(new AudioDeviceCallback() {
            @Override
            public void onAudioDevicesAdded(AudioDeviceInfo[] added) {
                report();
            }

            @Override
            public void onAudioDevicesRemoved(AudioDeviceInfo[] removed) {
                report();
            }
        }, main);
        // Another of the connected devices picked (in the output switcher): none is added or removed then.
        MediaRouter router = app.getSystemService(MediaRouter.class);
        if (router != null) {
            router.addCallback(MediaRouter.ROUTE_TYPE_LIVE_AUDIO, new MediaRouter.SimpleCallback() {
                @Override
                public void onRouteSelected(MediaRouter r, int type, MediaRouter.RouteInfo info) {
                    report();
                }

                @Override
                public void onRouteChanged(MediaRouter r, MediaRouter.RouteInfo info) {
                    report();
                }
            });
        }
        // The volume keys, the volume panel, another app.
        BroadcastReceiver volume = new BroadcastReceiver() {
            @Override
            public void onReceive(Context c, Intent intent) {
                report();
            }
        };
        if (Build.VERSION.SDK_INT >= 33) {
            app.registerReceiver(volume, new IntentFilter(VOLUME_CHANGED), Context.RECEIVER_NOT_EXPORTED);
        } else {
            app.registerReceiver(volume, new IntentFilter(VOLUME_CHANGED));
        }
        report();
    }

    /** Look again: the app is back on screen, maybe from the output switcher or the settings. */
    static void refresh() {
        if (audio != null) {
            report();
        }
    }

    private static void report() {
        AudioDeviceInfo device = current();
        int kind = device == null ? OutputLogic.PHONE : OutputLogic.kind(device.getType());
        CharSequence product = device == null ? null : device.getProductName();
        String name = OutputLogic.name(kind, product == null ? null : product.toString(), Build.MODEL);
        int[] steps = OutputLogic.steps(audio.getStreamVolume(AudioManager.STREAM_MUSIC), minVolume(), audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC), audio.isVolumeFixed());
        String line = OutputLogic.line(kind, name, steps[0], steps[1], offer);
        // The volume broadcast comes for every stream: the app hears only of a change.
        if (!line.equals(last)) {
            last = line;
            nativeOutput(line);
        }
    }

    /** The device music plays through: Android 13 says; before, the one its rules prefer of those connected. */
    private static AudioDeviceInfo current() {
        if (Build.VERSION.SDK_INT >= 33) {
            try {
                List<AudioDeviceInfo> devices = audio.getAudioDevicesForAttributes(MUSIC);
                if (!devices.isEmpty()) {
                    return devices.get(0);
                }
            } catch (RuntimeException e) {
                // Then by the rules below.
            }
        }
        AudioDeviceInfo[] outputs = audio.getDevices(AudioManager.GET_DEVICES_OUTPUTS);
        int[] types = new int[outputs.length];
        for (int i = 0; i < outputs.length; i++) {
            types[i] = outputs[i].getType();
        }
        int picked = OutputLogic.pick(types);
        return picked < 0 ? null : outputs[picked];
    }

    private static int minVolume() {
        return Build.VERSION.SDK_INT >= 28 ? audio.getStreamMinVolume(AudioManager.STREAM_MUSIC) : 0;
    }

    /** The media volume, to a step of the app's slider. */
    static void setVolume(int step) {
        if (audio == null || audio.isVolumeFixed()) {
            return;
        }
        try {
            audio.setStreamVolume(AudioManager.STREAM_MUSIC, OutputLogic.index(step, minVolume(), audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC)), 0);
        } catch (SecurityException e) {
            // Do Not Disturb doesn't let it be changed: the slider goes back to what it is.
        }
        report();
    }

    /** What the app's button will open on this phone: the first way that is there. */
    private static int offer(Context context) {
        for (int way : OutputLogic.switchers(Build.VERSION.SDK_INT)) {
            if (way == OutputLogic.ROUTER || intent(context, way) != null) {
                return OutputLogic.offer(way);
            }
        }
        return OutputLogic.OFFER_NOTHING;
    }

    /** Open Android's output switcher, or failing that the Bluetooth settings. False when nothing opened. */
    static boolean openSwitcher(Activity activity) {
        for (int way : OutputLogic.switchers(Build.VERSION.SDK_INT)) {
            try {
                if (way == OutputLogic.ROUTER) {
                    if (Build.VERSION.SDK_INT >= 34 && MediaRouter2.getInstance(activity).showSystemOutputSwitcher()) {
                        return true;
                    }
                    continue;
                }
                Intent intent = intent(activity, way);
                if (intent == null) {
                    continue;
                }
                if (way == OutputLogic.SYSTEM_UI) {
                    activity.sendBroadcast(intent);
                } else {
                    activity.startActivity(intent);
                }
                return true;
            } catch (RuntimeException e) {
                // Not this way, then.
            }
        }
        return false;
    }

    /** The intent that opens `way`, or null when no system app answers it on this phone. */
    private static Intent intent(Context context, int way) {
        PackageManager packages = context.getPackageManager();
        Intent intent;
        List<ResolveInfo> answers;
        if (way == OutputLogic.SYSTEM_UI) {
            intent = new Intent(SYSTEM_UI_SWITCHER).setPackage(SYSTEM_UI).putExtra(SYSTEM_UI_PACKAGE, context.getPackageName());
            answers = packages.queryBroadcastReceivers(intent, 0);
        } else if (way == OutputLogic.SETTINGS_PANEL) {
            intent = new Intent(SETTINGS_PANEL).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK).putExtra(SETTINGS_PANEL_PACKAGE, context.getPackageName());
            answers = packages.queryIntentActivities(intent, 0);
        } else if (way == OutputLogic.BLUETOOTH_SETTINGS) {
            intent = new Intent(Settings.ACTION_BLUETOOTH_SETTINGS);
            answers = packages.queryIntentActivities(intent, 0);
        } else {
            return null;
        }
        for (ResolveInfo answer : answers) {
            ApplicationInfo owner = answer.activityInfo == null ? null : answer.activityInfo.applicationInfo;
            if (owner != null && (owner.flags & (ApplicationInfo.FLAG_SYSTEM | ApplicationInfo.FLAG_UPDATED_SYSTEM_APP)) != 0) {
                return intent;
            }
        }
        return null;
    }
}
