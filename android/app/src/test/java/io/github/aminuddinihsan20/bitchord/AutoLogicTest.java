package io.github.aminuddinihsan20.bitchord;

import static io.github.aminuddinihsan20.bitchord.AutoLogic.NOTHING;
import static io.github.aminuddinihsan20.bitchord.AutoLogic.NO_APP;
import static io.github.aminuddinihsan20.bitchord.AutoLogic.READY;
import static io.github.aminuddinihsan20.bitchord.AutoLogic.SAY_NOTHING;
import static io.github.aminuddinihsan20.bitchord.AutoLogic.SAY_OPEN_APP;
import static io.github.aminuddinihsan20.bitchord.AutoLogic.SAY_SONG;
import static io.github.aminuddinihsan20.bitchord.AutoLogic.SEND;
import static io.github.aminuddinihsan20.bitchord.AutoLogic.STARTING;
import static io.github.aminuddinihsan20.bitchord.AutoLogic.WAIT;

import java.util.Arrays;
import java.util.List;

/**
 * What Android Auto's side decides, checked with nothing but a JDK: `sh android/test-java.sh`.
 */
public final class AutoLogicTest {
    private static final String F = "\u001f";
    private static final String AUTO = "com.google.android.projection.gearhead";

    private static int failures;

    public static void main(String[] args) {
        Runnable[] tests = {
            AutoLogicTest::without_the_app_there_is_nothing_to_browse,
            AutoLogicTest::an_app_that_has_not_published_yet_is_starting,
            AutoLogicTest::a_published_tree_is_ready,
            AutoLogicTest::a_ready_tree_is_sent_at_once,
            AutoLogicTest::without_the_app_a_load_gets_nothing_at_once,
            AutoLogicTest::a_starting_app_is_waited_for_a_few_seconds_and_no_longer,
            AutoLogicTest::android_auto_may_browse,
            AutoLogicTest::the_assistant_may_browse,
            AutoLogicTest::the_system_and_the_app_itself_may_browse,
            AutoLogicTest::another_app_may_not_browse,
            AutoLogicTest::a_name_alone_does_not_make_an_app_android_auto,
            AutoLogicTest::a_debug_build_lets_test_tools_in,
            AutoLogicTest::the_systems_resume_card_is_declined,
            AutoLogicTest::rows_are_read_one_a_line,
            AutoLogicTest::a_row_missing_a_field_is_skipped,
            AutoLogicTest::no_rows_is_an_empty_list,
            AutoLogicTest::a_node_whose_count_moved_has_changed,
            AutoLogicTest::a_new_tree_changes_every_node,
            AutoLogicTest::a_tree_that_is_gone_changes_every_node_it_had,
            AutoLogicTest::the_same_counts_change_nothing,
            AutoLogicTest::a_queue_rows_id_is_its_place,
            AutoLogicTest::what_is_not_a_queue_row_has_no_place,
            AutoLogicTest::the_cars_buttons_are_the_players_commands,
            AutoLogicTest::the_repeat_button_says_how_the_queue_repeats,
            AutoLogicTest::without_the_app_the_session_says_to_open_it,
            AutoLogicTest::with_the_app_the_session_shows_the_loaded_song_or_nothing,
            AutoLogicTest::the_line_of_what_plays_is_read_with_its_newer_fields_missing_or_there,
            AutoLogicTest::the_app_opened_by_a_voice_search_searches,
            AutoLogicTest::the_app_opened_to_play_plays,
            AutoLogicTest::the_app_opened_any_other_way_just_opens,
            AutoLogicTest::the_app_reopened_from_the_recent_apps_does_not_play_again,
        };
        int failedTests = 0;
        for (Runnable test : tests) {
            int before = failures;
            test.run();
            if (failures > before) {
                failedTests++;
            }
        }
        System.out.println((tests.length - failedTests) + " passed, " + failedTests + " failed");
        if (failedTests > 0) {
            System.exit(1);
        }
    }

    private static void without_the_app_there_is_nothing_to_browse() {
        same("no app", NO_APP, AutoLogic.state(false, ""));
        same("no app, whatever was published before it died", NO_APP, AutoLogic.state(false, "root" + F + "3"));
    }

    private static void an_app_that_has_not_published_yet_is_starting() {
        same("nothing published", STARTING, AutoLogic.state(true, ""));
        same("no answer", STARTING, AutoLogic.state(true, null));
    }

    private static void a_published_tree_is_ready() {
        same("published", READY, AutoLogic.state(true, "root" + F + "1"));
    }

    private static void a_ready_tree_is_sent_at_once() {
        same("ready", SEND, AutoLogic.load(READY, 0));
        same("ready, after a wait", SEND, AutoLogic.load(READY, 10_000));
    }

    private static void without_the_app_a_load_gets_nothing_at_once() {
        same("no app", NOTHING, AutoLogic.load(NO_APP, 0));
    }

    private static void a_starting_app_is_waited_for_a_few_seconds_and_no_longer() {
        same("just asked", WAIT, AutoLogic.load(STARTING, 0));
        same("two seconds on", WAIT, AutoLogic.load(STARTING, 2000));
        same("too long", NOTHING, AutoLogic.load(STARTING, AutoLogic.PATIENCE_MS));
    }

    private static void android_auto_may_browse() {
        same("installed by hand", true, AutoLogic.allowed(false, false, true, false, AUTO, false));
        same("part of the system", true, AutoLogic.allowed(false, false, true, true, AUTO, false));
    }

    private static void the_assistant_may_browse() {
        // The Google app, where a phone doesn't count it as part of the system.
        same("the Google app", true, AutoLogic.allowed(false, false, true, false, "com.google.android.googlequicksearchbox", false));
        same("its name alone", false, AutoLogic.allowed(false, false, false, false, "com.google.android.googlequicksearchbox", false));
    }

    private static void the_system_and_the_app_itself_may_browse() {
        same("the app", true, AutoLogic.allowed(true, false, false, false, "io.github.aminuddinihsan20.bitchord", false));
        same("the system", true, AutoLogic.allowed(false, true, false, false, "android", false));
        same("a system app", true, AutoLogic.allowed(false, false, true, true, "com.google.android.googlequicksearchbox", false));
    }

    private static void another_app_may_not_browse() {
        same("some app", false, AutoLogic.allowed(false, false, true, false, "com.example.curious", false));
        same("no name", false, AutoLogic.allowed(false, false, false, false, null, false));
    }

    private static void a_name_alone_does_not_make_an_app_android_auto() {
        // The name it gives is not the package of the caller's uid.
        same("a borrowed name", false, AutoLogic.allowed(false, false, false, false, AUTO, false));
        same("a borrowed system name", false, AutoLogic.allowed(false, false, false, true, "com.android.systemui", false));
    }

    private static void a_debug_build_lets_test_tools_in() {
        same("a media controller tester", true, AutoLogic.allowed(false, false, true, false, "com.example.android.mediacontroller", true));
    }

    private static void the_systems_resume_card_is_declined() {
        // Android's "resume" card plays through the service after the app is gone, which this app can't.
        same("asked for the recent song only", false, AutoLogic.serves(true));
        same("asked for the tree", true, AutoLogic.serves(false));
    }

    private static void rows_are_read_one_a_line() {
        List<AutoLogic.Row> rows = AutoLogic.rows("queue/0/v0" + F + "Tabun" + F + "YOASOBI" + F + "p\nqueue" + F + "Up next" + F + F + "b");
        same("two rows", 2, rows.size());
        same("id", "queue/0/v0", rows.get(0).id);
        same("title", "Tabun", rows.get(0).title);
        same("subtitle", "YOASOBI", rows.get(0).subtitle);
        same("plays", true, rows.get(0).playable);
        same("opens", false, rows.get(1).playable);
        same("no subtitle", "", rows.get(1).subtitle);
    }

    private static void a_row_missing_a_field_is_skipped() {
        List<AutoLogic.Row> rows = AutoLogic.rows("broken" + F + "row\npick/v1" + F + "Song" + F + "Artist" + F + "p\n" + F + "No id" + F + F + "p");
        same("one good row", 1, rows.size());
        same("the good one", "pick/v1", rows.get(0).id);
    }

    private static void no_rows_is_an_empty_list() {
        same("empty", 0, AutoLogic.rows("").size());
        same("nothing", 0, AutoLogic.rows(null).size());
    }

    private static void a_node_whose_count_moved_has_changed() {
        String before = "downloads" + F + "1\nqueue" + F + "4\nroot" + F + "2";
        String now = "downloads" + F + "1\nqueue" + F + "5\nroot" + F + "2";
        same("the queue", Arrays.asList("queue"), AutoLogic.changed(before, now));
    }

    private static void a_new_tree_changes_every_node() {
        same("all", Arrays.asList("queue", "root"), AutoLogic.changed("", "queue" + F + "1\nroot" + F + "1"));
    }

    private static void a_tree_that_is_gone_changes_every_node_it_had() {
        same("all", Arrays.asList("queue", "root"), AutoLogic.changed("queue" + F + "7\nroot" + F + "3", ""));
    }

    private static void the_same_counts_change_nothing() {
        String counts = "queue" + F + "7\nroot" + F + "3";
        same("none", Arrays.asList(), AutoLogic.changed(counts, counts));
        same("none of nothing", Arrays.asList(), AutoLogic.changed("", ""));
    }

    private static void a_queue_rows_id_is_its_place() {
        same("the first", 0L, AutoLogic.queueId("queue/0/dQw4w9WgXcQ"));
        same("a later one", 412L, AutoLogic.queueId("queue/412/a/b"));
    }

    private static void what_is_not_a_queue_row_has_no_place() {
        same("a download", -1L, AutoLogic.queueId("download/v1"));
        same("the node", -1L, AutoLogic.queueId("queue"));
        same("no number", -1L, AutoLogic.queueId("queue/x/v1"));
        same("no song", -1L, AutoLogic.queueId("queue/3"));
        same("nothing", -1L, AutoLogic.queueId(null));
    }

    private static void the_cars_buttons_are_the_players_commands() {
        same("shuffle", AutoLogic.SHUFFLE, AutoLogic.command(AutoLogic.ACTION_SHUFFLE));
        same("repeat", AutoLogic.REPEAT, AutoLogic.command(AutoLogic.ACTION_REPEAT));
        same("something else", 0, AutoLogic.command("com.example.OTHER"));
        same("nothing", 0, AutoLogic.command(null));
        // src/platform.rs (MediaCommand) counts them the same.
        same("the numbers", new int[] {1, 2, 3, 4, 5, 6, 7, 8},
                new int[] {AutoLogic.PLAY, AutoLogic.PAUSE, AutoLogic.TOGGLE, AutoLogic.NEXT, AutoLogic.PREVIOUS, AutoLogic.SEEK, AutoLogic.SHUFFLE, AutoLogic.REPEAT});
    }

    private static void the_repeat_button_says_how_the_queue_repeats() {
        same("off", "Repeat is off", AutoLogic.repeatLabel(0));
        same("all", "Repeating all", AutoLogic.repeatLabel(1));
        same("one", "Repeating one song", AutoLogic.repeatLabel(2));
        same("unknown", "Repeat is off", AutoLogic.repeatLabel(9));
    }

    private static void without_the_app_the_session_says_to_open_it() {
        same("no app", SAY_OPEN_APP, AutoLogic.idle(NO_APP, false));
        same("no app, whatever a dead one left", SAY_OPEN_APP, AutoLogic.idle(NO_APP, true));
    }

    private static void with_the_app_the_session_shows_the_loaded_song_or_nothing() {
        same("starting, nothing loaded", SAY_NOTHING, AutoLogic.idle(STARTING, false));
        same("ready, nothing loaded", SAY_NOTHING, AutoLogic.idle(READY, false));
        same("last time's song", SAY_SONG, AutoLogic.idle(READY, true));
        same("last time's song, tree still coming", SAY_SONG, AutoLogic.idle(STARTING, true));
    }

    private static void the_line_of_what_plays_is_read_with_its_newer_fields_missing_or_there() {
        AutoLogic.Song song = AutoLogic.song("Tabun" + F + "YOASOBI" + F + "https://x/y.jpg" + F + "1" + F + "261000" + F + "15500" + F + "125" + F + "2" + F + "4");
        same("title", "Tabun", song.title);
        same("artist", "YOASOBI", song.artist);
        same("cover", "https://x/y.jpg", song.cover);
        same("playing", true, song.playing);
        same("duration", 261000L, song.duration);
        same("position", 15500L, song.position);
        same("speed", 1.25f, song.speed);
        same("repeat", 2, song.repeat);
        same("place", 4L, song.queueIndex);
        AutoLogic.Song old = AutoLogic.song("Tabun" + F + "YOASOBI" + F + F + "0" + F + "261000" + F + "0");
        same("no speed told", 1f, old.speed);
        same("no repeat told", 0, old.repeat);
        same("no place told", -1L, old.queueIndex);
        same("paused", false, old.playing);
        same("nothing loaded", null, AutoLogic.song(""));
        same("no line", null, AutoLogic.song(null));
        same("too short", null, AutoLogic.song("Tabun" + F + "YOASOBI"));
    }

    private static final String PLAY_ACTION = "io.github.aminuddinihsan20.bitchord.OPEN_AND_PLAY";

    private static void the_app_opened_by_a_voice_search_searches() {
        same("play … on BitChord", AutoLogic.OPENED_TO_SEARCH, AutoLogic.opening("android.media.action.MEDIA_PLAY_FROM_SEARCH", PLAY_ACTION, false));
    }

    private static void the_app_opened_to_play_plays() {
        same("a widget's play", AutoLogic.OPENED_TO_PLAY, AutoLogic.opening(PLAY_ACTION, PLAY_ACTION, false));
    }

    private static void the_app_opened_any_other_way_just_opens() {
        same("its icon", AutoLogic.OPENED, AutoLogic.opening("android.intent.action.MAIN", PLAY_ACTION, false));
        same("no action", AutoLogic.OPENED, AutoLogic.opening(null, PLAY_ACTION, false));
    }

    private static void the_app_reopened_from_the_recent_apps_does_not_play_again() {
        // Android hands back the intent the app was first started with.
        same("to play", AutoLogic.OPENED, AutoLogic.opening(PLAY_ACTION, PLAY_ACTION, true));
        same("to search", AutoLogic.OPENED, AutoLogic.opening("android.media.action.MEDIA_PLAY_FROM_SEARCH", PLAY_ACTION, true));
    }

    private static void same(String what, Object expected, Object actual) {
        boolean equal = expected instanceof int[] ? Arrays.equals((int[]) expected, (int[]) actual) : expected == null ? actual == null : expected.equals(actual);
        if (!equal) {
            failures++;
            String caller = new Throwable().getStackTrace()[1].getMethodName();
            System.out.println("FAILED " + caller + " (" + what + "): expected " + show(expected) + ", got " + show(actual));
        }
    }

    private static String show(Object value) {
        return value instanceof int[] ? Arrays.toString((int[]) value) : String.valueOf(value);
    }
}
