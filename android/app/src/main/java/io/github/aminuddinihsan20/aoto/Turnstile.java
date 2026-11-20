package io.github.aminuddinihsan20.aoto;

import android.app.Activity;
import android.graphics.Color;
import android.os.Handler;
import android.os.Looper;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.webkit.JavascriptInterface;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.FrameLayout;

import org.json.JSONObject;

/**
 * The Better Lyrics API's Cloudflare Turnstile challenge in a WebView: the API's own
 * challenge page runs, and its widget reports the token. The view is 1 px and unseen
 * while the widget passes by itself; if Cloudflare wants an interaction it is shown in
 * the middle of the screen. Nothing here solves or forges the challenge.
 */
final class Turnstile {
    private static final String CHALLENGE = "https://lyrics.api.dacubeking.com/challenge";
    /** How long to wait for an unseen pass before showing the widget, and before giving up. */
    private static final long SHOW_AFTER_MS = 6_000, GIVE_UP_AFTER_MS = 120_000;

    /** The page posts to window.parent (itself, at top level): pass those messages on. */
    private static final String BRIDGE =
            "(function(){if(window.__aoto)return;window.__aoto=true;"
                    + "window.addEventListener('message',function(e){"
                    + "if(e.data&&typeof e.data.type==='string'&&e.data.type.indexOf('turnstile-')===0){"
                    + "Aoto.post(JSON.stringify(e.data));}});})();";

    private final Activity activity;
    private final Storage.Done done;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private WebView web;
    private boolean finished;

    private Turnstile(Activity activity, Storage.Done done) {
        this.activity = activity;
        this.done = done;
    }

    /** Run the challenge; `done` gets the token, or why not. Call on the main thread. */
    static void run(Activity activity, Storage.Done done) {
        new Turnstile(activity, done).start();
    }

    private void start() {
        web = new WebView(activity);
        web.setBackgroundColor(Color.TRANSPARENT);
        WebSettings settings = web.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        web.addJavascriptInterface(new Object() {
            @JavascriptInterface
            public void post(final String json) {
                handler.post(() -> message(json));
            }
        }, "Aoto");
        web.setWebViewClient(new WebViewClient() {
            @Override
            public void onPageStarted(WebView view, String url, android.graphics.Bitmap favicon) {
                view.evaluateJavascript(BRIDGE, null);
            }

            @Override
            public void onPageFinished(WebView view, String url) {
                view.evaluateJavascript(BRIDGE, null);
            }
        });
        // Attached (a detached WebView doesn't run its page reliably), but out of sight.
        FrameLayout.LayoutParams unseen = new FrameLayout.LayoutParams(1, 1, Gravity.TOP | Gravity.START);
        activity.addContentView(web, unseen);
        web.setAlpha(0f);
        web.loadUrl(CHALLENGE);
        handler.postDelayed(this::show, SHOW_AFTER_MS);
        handler.postDelayed(() -> finish(false, "timed out waiting for verification"), GIVE_UP_AFTER_MS);
    }

    /** Cloudflare wants an interaction: let the person see the widget. */
    private void show() {
        if (finished || web == null) {
            return;
        }
        int w = dp(320), h = dp(120);
        FrameLayout.LayoutParams seen = new FrameLayout.LayoutParams(w, h, Gravity.CENTER);
        web.setLayoutParams(seen);
        web.setBackgroundColor(Color.WHITE);
        web.setAlpha(1f);
    }

    private int dp(int v) {
        return (int) TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v, activity.getResources().getDisplayMetrics());
    }

    private void message(String json) {
        if (finished) {
            return;
        }
        try {
            JSONObject o = new JSONObject(json);
            String type = o.optString("type");
            if ("turnstile-token".equals(type)) {
                String token = o.optString("token");
                finish(!token.isEmpty(), token.isEmpty() ? "empty token" : token);
            } else if ("turnstile-error".equals(type)) {
                finish(false, "challenge error " + o.optString("error"));
            } else if ("turnstile-timeout".equals(type)) {
                finish(false, "challenge timed out");
            } else if ("turnstile-expired".equals(type) && web != null) {
                // Expired before use: ask the widget for a fresh one.
                web.evaluateJavascript("turnstile.reset()", null);
            }
        } catch (Exception e) {
            finish(false, "unreadable challenge message");
        }
    }

    private void finish(boolean ok, String text) {
        if (finished) {
            return;
        }
        finished = true;
        handler.removeCallbacksAndMessages(null);
        if (web != null) {
            ViewGroup parent = (ViewGroup) web.getParent();
            if (parent != null) {
                parent.removeView(web);
            }
            web.setVisibility(View.GONE);
            web.destroy();
            web = null;
        }
        done.finish(ok, text);
    }
}
