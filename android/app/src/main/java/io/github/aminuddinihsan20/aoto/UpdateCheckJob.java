package io.github.aminuddinihsan20.aoto;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.job.JobInfo;
import android.app.job.JobParameters;
import android.app.job.JobScheduler;
import android.app.job.JobService;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.net.Uri;
import android.util.Log;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;

/**
 * Looks for a newer release twice a day, with the app closed or open, and says so in a
 * notification: tapping it downloads the APK, which installs over this version (the same
 * signing key), so nothing has to be uninstalled first. Told once per version. It touches
 * nothing of the player; a failure here only means no notification this time.
 */
public final class UpdateCheckJob extends JobService {
    private static final String TAG = "AotoUpdate";
    static final int JOB_ID = 7001;
    private static final long PERIOD_MS = 12L * 60 * 60 * 1000;
    private static final String FEED = "https://api.github.com/repos/aminuddinihsan20/aoto/releases/latest";
    private static final String CHANNEL = "updates";
    private static final String PREFS = "aoto_updates";
    private static final String ANNOUNCED = "announced";
    private static final int NOTIFICATION_ID = 7002;

    private volatile Thread worker;

    /** Make sure the periodic check is set up; harmless to call at every start. */
    static void schedule(Context context) {
        try {
            JobScheduler scheduler = context.getSystemService(JobScheduler.class);
            if (scheduler == null || scheduler.getPendingJob(JOB_ID) != null) {
                return;
            }
            JobInfo job = new JobInfo.Builder(JOB_ID, new ComponentName(context, UpdateCheckJob.class))
                .setPeriodic(PERIOD_MS)
                .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                .setPersisted(true)
                .build();
            scheduler.schedule(job);
        } catch (RuntimeException e) {
            // No check is better than no app: nothing here may stop it starting.
            Log.w(TAG, "couldn't schedule the update check", e);
        }
    }

    @Override
    public boolean onStartJob(JobParameters params) {
        worker = new Thread(() -> {
            try {
                check(getApplicationContext());
            } catch (Exception e) {
                Log.w(TAG, "update check failed: " + e);
            } finally {
                jobFinished(params, false);
            }
        }, "aoto-update-check");
        worker.start();
        return true;
    }

    @Override
    public boolean onStopJob(JobParameters params) {
        Thread t = worker;
        if (t != null) {
            t.interrupt();
        }
        // The next period asks again.
        return false;
    }

    private static void check(Context context) throws Exception {
        String current = context.getPackageManager().getPackageInfo(context.getPackageName(), 0).versionName;
        JSONObject release = new JSONObject(get(FEED));
        JSONArray assets = release.optJSONArray("assets");
        int n = assets == null ? 0 : assets.length();
        String[] names = new String[n];
        String[] urls = new String[n];
        for (int i = 0; i < n; i++) {
            JSONObject a = assets.getJSONObject(i);
            names[i] = a.optString("name", null);
            urls[i] = a.optString("browser_download_url", null);
        }
        String[] offer = UpdateLogic.offer(release.optString("tag_name", null), current, names, urls);
        if (offer == null) {
            Log.i(TAG, current + " is current");
            return;
        }
        SharedPreferences prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
        if (!UpdateLogic.shouldAnnounce(offer[0], prefs.getString(ANNOUNCED, null))) {
            return;
        }
        if (announce(context, offer[0], offer[1])) {
            prefs.edit().putString(ANNOUNCED, offer[0]).apply();
        }
    }

    /** The notification; false when notifications are off, so it is tried again later. */
    private static boolean announce(Context context, String version, String url) {
        NotificationManager manager = context.getSystemService(NotificationManager.class);
        if (manager == null || !manager.areNotificationsEnabled()) {
            return false;
        }
        manager.createNotificationChannel(new NotificationChannel(CHANNEL, "App updates", NotificationManager.IMPORTANCE_DEFAULT));
        Intent open = new Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        PendingIntent tap = PendingIntent.getActivity(context, 0, open, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        Notification note = new Notification.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_music)
            .setContentTitle(UpdateLogic.title(version))
            .setContentText(UpdateLogic.text())
            .setStyle(new Notification.BigTextStyle().bigText(UpdateLogic.text()))
            .setContentIntent(tap)
            .setAutoCancel(true)
            .build();
        manager.notify(NOTIFICATION_ID, note);
        Log.i(TAG, version + " announced");
        return true;
    }

    private static String get(String address) throws Exception {
        HttpURLConnection http = (HttpURLConnection) new URL(address).openConnection();
        http.setConnectTimeout(15_000);
        http.setReadTimeout(15_000);
        http.setRequestProperty("Accept", "application/vnd.github+json");
        http.setRequestProperty("User-Agent", "Aoto-update-check");
        try {
            if (http.getResponseCode() != 200) {
                throw new IllegalStateException("HTTP " + http.getResponseCode());
            }
            try (InputStream in = http.getInputStream()) {
                ByteArrayOutputStream out = new ByteArrayOutputStream();
                byte[] buf = new byte[8192];
                int read;
                while ((read = in.read(buf)) > 0 && out.size() < 2_000_000) {
                    out.write(buf, 0, read);
                }
                return out.toString("UTF-8");
            }
        } finally {
            http.disconnect();
        }
    }
}
