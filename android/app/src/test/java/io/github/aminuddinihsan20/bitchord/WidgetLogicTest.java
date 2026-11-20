package io.github.aminuddinihsan20.bitchord;

import static io.github.aminuddinihsan20.bitchord.WidgetLogic.BODY;
import static io.github.aminuddinihsan20.bitchord.WidgetLogic.COMMAND;
import static io.github.aminuddinihsan20.bitchord.WidgetLogic.HIDDEN;
import static io.github.aminuddinihsan20.bitchord.WidgetLogic.NEXT;
import static io.github.aminuddinihsan20.bitchord.WidgetLogic.NO_PROGRESS;
import static io.github.aminuddinihsan20.bitchord.WidgetLogic.OPEN;
import static io.github.aminuddinihsan20.bitchord.WidgetLogic.OPEN_AND_PLAY;
import static io.github.aminuddinihsan20.bitchord.WidgetLogic.PREVIOUS;
import static io.github.aminuddinihsan20.bitchord.WidgetLogic.TOGGLE;

import java.util.Arrays;

/**
 * The widgets' decisions, checked with nothing but a JDK: `sh android/test-java.sh`. (The
 * project has no Java test library; each check here is a method named for what it says.)
 */
public final class WidgetLogicTest {
    private static int failures;

    public static void main(String[] args) {
        Runnable[] tests = {
            WidgetLogicTest::the_body_always_opens_the_app,
            WidgetLogicTest::with_a_song_showing_the_buttons_tell_the_player,
            WidgetLogicTest::with_no_song_showing_play_opens_the_app_to_play_and_skipping_is_not_offered,
            WidgetLogicTest::a_button_left_behind_by_a_dead_app_opens_it_and_only_play_plays,
            WidgetLogicTest::progress_is_the_percent_played,
            WidgetLogicTest::progress_is_unknown_without_a_length,
            WidgetLogicTest::progress_stays_within_the_bar,
            WidgetLogicTest::the_bar_moves_a_percent_at_a_time_and_no_more_often_than_every_two_seconds,
            WidgetLogicTest::a_cover_is_made_no_larger_than_the_widget_shows_it,
            WidgetLogicTest::a_cover_is_capped_however_dense_the_screen,
            WidgetLogicTest::a_cover_stays_a_sixth_of_what_a_small_screen_allows_a_widget,
            WidgetLogicTest::a_wide_cover_is_cropped_to_a_square_from_its_middle,
            WidgetLogicTest::a_cover_is_cropped_to_the_shape_of_the_widget_it_fills,
            WidgetLogicTest::a_widget_of_unknown_size_gets_a_square,
            WidgetLogicTest::a_cropped_cover_keeps_the_widgets_shape_at_its_longest_side,
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

    private static void the_body_always_opens_the_app() {
        same("body, song showing", OPEN, WidgetLogic.tap(BODY, true));
        same("body, no song", OPEN, WidgetLogic.tap(BODY, false));
    }

    private static void with_a_song_showing_the_buttons_tell_the_player() {
        same("play", COMMAND, WidgetLogic.tap(TOGGLE, true));
        same("previous", COMMAND, WidgetLogic.tap(PREVIOUS, true));
        same("next", COMMAND, WidgetLogic.tap(NEXT, true));
    }

    private static void with_no_song_showing_play_opens_the_app_to_play_and_skipping_is_not_offered() {
        same("play", OPEN_AND_PLAY, WidgetLogic.tap(TOGGLE, false));
        same("previous", HIDDEN, WidgetLogic.tap(PREVIOUS, false));
        same("next", HIDDEN, WidgetLogic.tap(NEXT, false));
    }

    private static void a_button_left_behind_by_a_dead_app_opens_it_and_only_play_plays() {
        same("play", OPEN_AND_PLAY, WidgetLogic.coldTap(TOGGLE));
        same("previous", OPEN, WidgetLogic.coldTap(PREVIOUS));
        same("next", OPEN, WidgetLogic.coldTap(NEXT));
        same("body", OPEN, WidgetLogic.coldTap(BODY));
    }

    private static void progress_is_the_percent_played() {
        same("start", 0, WidgetLogic.percent(0, 200_000));
        same("a quarter", 25, WidgetLogic.percent(50_000, 200_000));
        same("just short of a percent", 0, WidgetLogic.percent(1_999, 200_000));
        same("the end", 100, WidgetLogic.percent(200_000, 200_000));
    }

    private static void progress_is_unknown_without_a_length() {
        same("no length", NO_PROGRESS, WidgetLogic.percent(5_000, 0));
        same("a negative length", NO_PROGRESS, WidgetLogic.percent(5_000, -1));
    }

    private static void progress_stays_within_the_bar() {
        same("past the end", 100, WidgetLogic.percent(250_000, 200_000));
        same("before the start", 0, WidgetLogic.percent(-10, 200_000));
        same("a length beyond int", 50, WidgetLogic.percent(5_000_000_000L, 10_000_000_000L));
    }

    private static void the_bar_moves_a_percent_at_a_time_and_no_more_often_than_every_two_seconds() {
        same("same percent", false, WidgetLogic.progressDue(40, 40, 60_000));
        same("a new percent, long after", true, WidgetLogic.progressDue(40, 41, 2_000));
        same("a new percent, too soon", false, WidgetLogic.progressDue(40, 41, 1_999));
        same("back to an earlier one", true, WidgetLogic.progressDue(40, 12, 5_000));
        same("the length is gone", false, WidgetLogic.progressDue(40, NO_PROGRESS, 5_000));
    }

    private static void a_cover_is_made_no_larger_than_the_widget_shows_it() {
        same("a pill's cover", 132, WidgetLogic.coverSide(132, 1080L * 2400));
        same("nothing to show", 1, WidgetLogic.coverSide(0, 1080L * 2400));
    }

    private static void a_cover_is_capped_however_dense_the_screen() {
        same("a 4x dense tablet", 512, WidgetLogic.coverSide(720, 2560L * 1600));
    }

    private static void a_cover_stays_a_sixth_of_what_a_small_screen_allows_a_widget() {
        // The system allows a widget's update 6 bytes a screen pixel (1.5 screens of 4-byte
        // pixels): a sixth of that is a byte a pixel, and a cover has 4 bytes a pixel.
        int side = WidgetLogic.coverSide(400, 480L * 800);
        same("480x800", 309, side);
        same("within the sixth", true, 4L * side * side <= 480L * 800);
    }

    private static void a_wide_cover_is_cropped_to_a_square_from_its_middle() {
        same("16:9 video art", new int[] {140, 0, 360, 360}, WidgetLogic.crop(640, 360, 1, 1));
        same("tall art", new int[] {0, 50, 300, 300}, WidgetLogic.crop(300, 400, 1, 1));
        same("already square", new int[] {0, 0, 512, 512}, WidgetLogic.crop(512, 512, 180, 180));
    }

    private static void a_cover_is_cropped_to_the_shape_of_the_widget_it_fills() {
        same("a widget taller than wide", new int[] {50, 0, 400, 500}, WidgetLogic.crop(500, 500, 160, 200));
        same("a widget wider than tall", new int[] {0, 125, 500, 250}, WidgetLogic.crop(500, 500, 300, 150));
    }

    private static void a_widget_of_unknown_size_gets_a_square() {
        same("no size", new int[] {0, 0, 500, 500}, WidgetLogic.crop(500, 500, 0, 0));
        same("no height", new int[] {70, 0, 360, 360}, WidgetLogic.crop(500, 360, 200, 0));
        same("fit, no size", new int[] {300, 300}, WidgetLogic.fit(0, 0, 300));
    }

    private static void a_cropped_cover_keeps_the_widgets_shape_at_its_longest_side() {
        same("taller than wide", new int[] {240, 300}, WidgetLogic.fit(160, 200, 300));
        same("wider than tall", new int[] {300, 150}, WidgetLogic.fit(300, 150, 300));
        same("square", new int[] {300, 300}, WidgetLogic.fit(180, 180, 300));
        same("a sliver still has a pixel", new int[] {300, 1}, WidgetLogic.fit(4000, 1, 300));
    }

    private static void same(String what, Object expected, Object actual) {
        boolean equal = expected instanceof int[] ? Arrays.equals((int[]) expected, (int[]) actual) : expected.equals(actual);
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
