package io.github.aminuddinihsan20.aoto;

/**
 * When a newer release is offered, and when the listener is told about it, checked with
 * nothing but a JDK: `sh android/test-java.sh`.
 */
public final class UpdateLogicTest {
    private static int failures;

    public static void main(String[] args) {
        Runnable[] tests = {
            UpdateLogicTest::a_higher_number_is_newer_whatever_its_length,
            UpdateLogicTest::the_same_version_is_not_newer_and_a_leading_v_does_not_count,
            UpdateLogicTest::a_newer_release_with_its_apk_is_offered,
            UpdateLogicTest::the_release_that_is_installed_is_not_offered,
            UpdateLogicTest::an_older_release_is_not_offered,
            UpdateLogicTest::a_release_without_an_apk_is_not_offered,
            UpdateLogicTest::only_the_apk_of_this_release_counts,
            UpdateLogicTest::only_an_https_download_is_offered,
            UpdateLogicTest::a_version_is_announced_once,
            UpdateLogicTest::the_text_says_nothing_has_to_be_uninstalled,
        };
        for (Runnable t : tests) {
            t.run();
        }
        System.out.println((tests.length - failures) + " passed, " + failures + " failed");
        if (failures > 0) {
            System.exit(1);
        }
    }

    private static void check(boolean ok, String what) {
        if (!ok) {
            failures++;
            System.out.println("FAILED " + what);
        }
    }

    private static final String APK = "https://github.com/aminuddinihsan20/aoto/releases/download/v1.1.0/aoto-1.1.0-android-arm64.apk";

    private static String[] offer(String tag, String current, String[] names, String[] urls) {
        return UpdateLogic.offer(tag, current, names, urls);
    }

    private static void a_higher_number_is_newer_whatever_its_length() {
        check(UpdateLogic.newer("1.0.2", "1.0.1"), "1.0.2 > 1.0.1");
        check(UpdateLogic.newer("1.1", "1.0.9"), "1.1 > 1.0.9");
        check(UpdateLogic.newer("1.0.10", "1.0.9"), "1.0.10 > 1.0.9, not compared as text");
        check(UpdateLogic.newer("2", "1.9.9"), "2 > 1.9.9");
        check(!UpdateLogic.newer("1.0.1", "1.0.2"), "1.0.1 < 1.0.2");
    }

    private static void the_same_version_is_not_newer_and_a_leading_v_does_not_count() {
        check(!UpdateLogic.newer("1.0.1", "1.0.1"), "same");
        check(!UpdateLogic.newer("v1.0.1", "1.0.1"), "v1.0.1 is 1.0.1");
        check(!UpdateLogic.newer("1.0", "1.0.0"), "missing parts are zero");
        check(UpdateLogic.newer("v1.0.2", "1.0.1"), "v1.0.2 > 1.0.1");
    }

    private static void a_newer_release_with_its_apk_is_offered() {
        String[] got = offer("v1.1.0", "1.0.1", new String[] {"aoto-1.1.0-android-arm64.apk"}, new String[] {APK});
        check(got != null && got[0].equals("1.1.0") && got[1].equals(APK), "offer 1.1.0");
    }

    private static void the_release_that_is_installed_is_not_offered() {
        check(offer("v1.0.1", "1.0.1", new String[] {"aoto-1.0.1-android-arm64.apk"}, new String[] {APK}) == null, "same version");
    }

    private static void an_older_release_is_not_offered() {
        check(offer("v1.0.0", "1.0.1", new String[] {"aoto-1.0.0-android-arm64.apk"}, new String[] {APK}) == null, "older");
    }

    private static void a_release_without_an_apk_is_not_offered() {
        check(offer("v1.1.0", "1.0.1", new String[0], new String[0]) == null, "no assets");
        check(offer("v1.1.0", "1.0.1", new String[] {"notes.txt"}, new String[] {APK}) == null, "no apk among the assets");
    }

    private static void only_the_apk_of_this_release_counts() {
        // A release that carries another version's file is not an update to that file.
        check(offer("v1.1.0", "1.0.1", new String[] {"aoto-1.0.1-android-arm64.apk"}, new String[] {APK}) == null, "another version's apk");
    }

    private static void only_an_https_download_is_offered() {
        check(offer("v1.1.0", "1.0.1", new String[] {"aoto-1.1.0-android-arm64.apk"}, new String[] {"http://example.com/a.apk"}) == null, "http");
        check(offer("v1.1.0", "1.0.1", new String[] {"aoto-1.1.0-android-arm64.apk"}, new String[] {null}) == null, "no url");
    }

    private static void a_version_is_announced_once() {
        check(UpdateLogic.shouldAnnounce("1.1.0", null), "nothing announced yet");
        check(UpdateLogic.shouldAnnounce("1.1.0", ""), "empty record");
        check(!UpdateLogic.shouldAnnounce("1.1.0", "1.1.0"), "already told");
        check(UpdateLogic.shouldAnnounce("1.2.0", "1.1.0"), "a still newer one is told again");
    }

    private static void the_text_says_nothing_has_to_be_uninstalled() {
        check(UpdateLogic.title("1.1.0").equals("Aoto 1.1.0 is available"), "title");
        check(UpdateLogic.text().toLowerCase().contains("uninstall"), "says it installs over the old one");
    }
}
