package io.github.aminuddinihsan20.aoto;

import android.app.Activity;
import android.graphics.Color;
import android.os.Bundle;
import android.view.ViewGroup;
import android.webkit.CookieManager;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;

import androidx.webkit.WebSettingsCompat;
import androidx.webkit.WebViewFeature;

import java.util.HashSet;

/**
 * "Sign in with Google" and "Sign in to Spotify": the service's own sign-in page in a
 * WebView, until the flow lands on the service's home page signed in. Then the session
 * cookie goes to the app (what the desktop's sign-in window finds the same way) and the
 * WebView's cookies are wiped, so the next sign-in starts fresh.
 */
public class LoginActivity extends Activity {
    static final String EXTRA_SITE = "site";

    private static final int GOOGLE = 1;

    /** Reports to the app: ok with the cookie, or not ok with why ("cancelled" if closed). */
    private static native void nativeLoginResult(boolean ok, String text);

    // A mobile Chrome's user agent without WebView's "; wv" marker, which Google's sign-in
    // refuses ("this browser may not be secure").
    private static final String USER_AGENT =
            "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Mobile Safari/537.36";

    private String home;
    private String sessionCookie;
    private boolean google;
    private boolean answered;
    private WebView web;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        google = getIntent().getIntExtra(EXTRA_SITE, GOOGLE) == GOOGLE;
        String start;
        if (google) {
            start = "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fmusic.youtube.com%2F";
            home = "https://music.youtube.com/";
            sessionCookie = "SAPISID";
        } else {
            start = "https://accounts.spotify.com/en/login?continue=https%3A%2F%2Fopen.spotify.com%2F";
            home = "https://open.spotify.com/";
            sessionCookie = "sp_dc";
        }

        web = new WebView(this);
        web.setBackgroundColor(Color.BLACK);
        setContentView(web, new ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));

        WebSettings settings = web.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setUserAgentString(USER_AGENT);
        // WebView names the app in an X-Requested-With header, another giveaway.
        if (WebViewFeature.isFeatureSupported(WebViewFeature.REQUESTED_WITH_HEADER_ALLOW_LIST)) {
            WebSettingsCompat.setRequestedWithHeaderOriginAllowList(settings, new HashSet<>());
        }

        CookieManager cookies = CookieManager.getInstance();
        cookies.setAcceptCookie(true);
        cookies.setAcceptThirdPartyCookies(web, true);
        // Start signed out: whoever was signed in here before is not this person.
        cookies.removeAllCookies(ok -> web.loadUrl(start));

        web.setWebViewClient(new WebViewClient() {
            @Override
            public void onPageFinished(WebView view, String url) {
                if (url != null && url.startsWith(home)) {
                    check();
                }
            }
        });
    }

    /** Landing on the home page also happens signed out: only the session cookie says it. */
    private void check() {
        String all = CookieManager.getInstance().getCookie(home);
        if (all == null || !all.contains(sessionCookie + "=")) {
            return;
        }
        String result = all;
        if (!google) {
            // Spotify's transfer needs just the sp_dc cookie's value.
            result = null;
            for (String part : all.split(";")) {
                String p = part.trim();
                if (p.startsWith("sp_dc=")) {
                    result = p.substring("sp_dc=".length());
                }
            }
            if (result == null) {
                return;
            }
        }
        answer(true, result);
        CookieManager.getInstance().removeAllCookies(null);
        finish();
    }

    private void answer(boolean ok, String text) {
        if (!answered) {
            answered = true;
            nativeLoginResult(ok, text);
        }
    }

    @Override
    public void onBackPressed() {
        if (web != null && web.canGoBack()) {
            web.goBack();
        } else {
            super.onBackPressed();
        }
    }

    @Override
    protected void onDestroy() {
        // Closed without signing in (Back, or the system ending the activity).
        answer(false, "cancelled");
        if (web != null) {
            web.destroy();
        }
        super.onDestroy();
    }
}
