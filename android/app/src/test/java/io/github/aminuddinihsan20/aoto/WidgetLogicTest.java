package io.github.aminuddinihsan20.aoto;

import static io.github.aminuddinihsan20.aoto.WidgetLogic.BODY;
import static io.github.aminuddinihsan20.aoto.WidgetLogic.COMMAND;
import static io.github.aminuddinihsan20.aoto.WidgetLogic.COUNTS;
import static io.github.aminuddinihsan20.aoto.WidgetLogic.HIDDEN;
import static io.github.aminuddinihsan20.aoto.WidgetLogic.NEXT;
import static io.github.aminuddinihsan20.aoto.WidgetLogic.NO_PROGRESS;
import static io.github.aminuddinihsan20.aoto.WidgetLogic.NO_TIME;
import static io.github.aminuddinihsan20.aoto.WidgetLogic.OPEN;
import static io.github.aminuddinihsan20.aoto.WidgetLogic.OPEN_AND_PLAY;
import static io.github.aminuddinihsan20.aoto.WidgetLogic.PREVIOUS;
import static io.github.aminuddinihsan20.aoto.WidgetLogic.ROW;
import static io.github.aminuddinihsan20.aoto.WidgetLogic.STANDS;
import static io.github.aminuddinihsan20.aoto.WidgetLogic.TOGGLE;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

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
            WidgetLogicTest::the_wide_widget_is_padded_as_designed_and_less_when_low,
            WidgetLogicTest::the_wide_widgets_cover_is_as_tall_as_the_widget_has_room_for,
            WidgetLogicTest::the_wide_widgets_cover_leaves_room_for_three_full_buttons,
            WidgetLogicTest::the_wide_widgets_cover_is_neither_tiny_nor_larger_than_designed,
            WidgetLogicTest::a_wide_widget_of_unknown_size_gets_a_middling_cover,
            WidgetLogicTest::the_artist_is_left_out_of_a_wide_widget_too_low_for_two_lines,
            WidgetLogicTest::the_times_are_left_out_of_a_wide_widget_before_the_artist_is,
            WidgetLogicTest::a_time_reads_as_the_systems_ticking_label_does,
            WidgetLogicTest::the_time_played_counts_by_itself_only_when_it_can_be_right,
            WidgetLogicTest::the_mini_widget_gives_up_its_cover_before_its_buttons,
            WidgetLogicTest::the_queue_widget_shows_the_rows_it_has_room_for,
            WidgetLogicTest::up_next_is_what_follows_the_song_that_plays,
            WidgetLogicTest::up_next_never_offers_the_song_that_plays_or_one_before_it,
            WidgetLogicTest::up_next_has_only_rows_of_the_queue,
            WidgetLogicTest::a_queue_row_is_told_to_the_player_and_is_not_there_without_one,
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

    private static void the_wide_widget_is_padded_as_designed_and_less_when_low() {
        same("the designed size", 16, WidgetLogic.widePadDp(180));
        same("just high enough for everything", 16, WidgetLogic.widePadDp(152));
        same("just too low", 12, WidgetLogic.widePadDp(151));
        same("two cells by the old rule", 12, WidgetLogic.widePadDp(110));
        same("unknown", 16, WidgetLogic.widePadDp(0));
    }

    private static void the_wide_widgets_cover_is_as_tall_as_the_widget_has_room_for() {
        same("a low widget, 12dp above and below", 86, WidgetLogic.wideCoverDp(380, 110));
        same("the designed size, 16dp above and below", 148, WidgetLogic.wideCoverDp(380, 180));
    }

    private static void the_wide_widgets_cover_leaves_room_for_three_full_buttons() {
        // Beside it: 32dp of padding, a 16dp gap, three buttons at least 48dp wide, 8dp apart.
        same("a 360dp phone's four cells", 120, WidgetLogic.wideCoverDp(328, 190));
        same("four cells of five", 88, WidgetLogic.wideCoverDp(296, 190));
    }

    private static void the_wide_widgets_cover_is_neither_tiny_nor_larger_than_designed() {
        same("a huge cell", 148, WidgetLogic.wideCoverDp(600, 400));
        same("narrower than the buttons need", 40, WidgetLogic.wideCoverDp(200, 190));
        same("lower than a button", 40, WidgetLogic.wideCoverDp(380, 60));
    }

    private static void a_wide_widget_of_unknown_size_gets_a_middling_cover() {
        same("no size", 96, WidgetLogic.wideCoverDp(0, 0));
        same("no height", 96, WidgetLogic.wideCoverDp(380, 0));
    }

    private static void the_artist_is_left_out_of_a_wide_widget_too_low_for_two_lines() {
        same("two cells by the old rule", false, WidgetLogic.roomForArtist(110));
        same("just too low", false, WidgetLogic.roomForArtist(125));
        same("just enough", true, WidgetLogic.roomForArtist(126));
        same("the designed size", true, WidgetLogic.roomForArtist(180));
        same("unknown", true, WidgetLogic.roomForArtist(0));
    }

    private static void the_times_are_left_out_of_a_wide_widget_before_the_artist_is() {
        same("room for the artist only", false, WidgetLogic.roomForTimes(126));
        same("just too low", false, WidgetLogic.roomForTimes(143));
        same("just enough", true, WidgetLogic.roomForTimes(144));
        same("the designed size", true, WidgetLogic.roomForTimes(180));
        same("unknown", true, WidgetLogic.roomForTimes(0));
    }

    private static void a_time_reads_as_the_systems_ticking_label_does() {
        same("under ten minutes", "04:02", WidgetLogic.time(242_000));
        same("part of a second", "01:24", WidgetLogic.time(84_999));
        same("nothing yet", "00:00", WidgetLogic.time(0));
        same("never below nothing", "00:00", WidgetLogic.time(-5));
        same("over an hour", "1:02:03", WidgetLogic.time(3_723_000));
    }

    private static void the_time_played_counts_by_itself_only_when_it_can_be_right() {
        same("playing as recorded", COUNTS, WidgetLogic.elapsed(true, 1f));
        same("paused", STANDS, WidgetLogic.elapsed(false, 1f));
        same("paused, whatever the speed", STANDS, WidgetLogic.elapsed(false, 1.5f));
        // The system's label counts a second a second: wrong for a song played faster or slower.
        same("playing faster", NO_TIME, WidgetLogic.elapsed(true, 1.25f));
        same("playing slower", NO_TIME, WidgetLogic.elapsed(true, 0.75f));
    }

    private static void the_mini_widget_gives_up_its_cover_before_its_buttons() {
        same("the designed size", 48, WidgetLogic.miniCoverDp(182));
        same("just room for all of it", 48, WidgetLogic.miniCoverDp(180));
        same("a little narrow: a smaller cover", 38, WidgetLogic.miniCoverDp(170));
        same("too narrow for a cover worth showing", 0, WidgetLogic.miniCoverDp(160));
        same("unknown", 48, WidgetLogic.miniCoverDp(0));
    }

    private static void the_queue_widget_shows_the_rows_it_has_room_for() {
        same("the designed size", 3, WidgetLogic.queueRows(276));
        same("a row's height short", 2, WidgetLogic.queueRows(275));
        same("two cells high", 0, WidgetLogic.queueRows(150));
        same("a cell higher", 5, WidgetLogic.queueRows(372));
        same("as high as the screen", 6, WidgetLogic.queueRows(900));
        same("unknown", 3, WidgetLogic.queueRows(0));
    }

    private static List<AutoLogic.Row> queue(String... ids) {
        List<AutoLogic.Row> rows = new ArrayList<>();
        for (String id : ids) {
            rows.add(new AutoLogic.Row(id, "Song " + id, "Artist", true));
        }
        return rows;
    }

    private static String ids(List<AutoLogic.Row> rows) {
        StringBuilder all = new StringBuilder();
        for (AutoLogic.Row row : rows) {
            all.append(row.id).append(' ');
        }
        return all.toString().trim();
    }

    private static void up_next_is_what_follows_the_song_that_plays() {
        // The app's list starts with the song that plays (src/auto.rs).
        List<AutoLogic.Row> rows = queue("queue/4/a", "queue/5/b", "queue/6/c", "queue/7/d", "queue/8/e");
        same("three of them", "queue/5/b queue/6/c queue/7/d", ids(WidgetLogic.upNext(rows, 4, 3)));
        same("room for more than there are", "queue/5/b queue/6/c queue/7/d queue/8/e", ids(WidgetLogic.upNext(rows, 4, 6)));
        same("no room", "", ids(WidgetLogic.upNext(rows, 4, 0)));
    }

    private static void up_next_never_offers_the_song_that_plays_or_one_before_it() {
        List<AutoLogic.Row> rows = queue("queue/4/a", "queue/5/b", "queue/6/c");
        // The player moved on before the list was read again.
        same("a list a song behind", "queue/6/c", ids(WidgetLogic.upNext(rows, 5, 3)));
        same("the place isn't known: the first row is the song", "queue/5/b queue/6/c", ids(WidgetLogic.upNext(rows, -1, 3)));
        same("the last song", "", ids(WidgetLogic.upNext(queue("queue/9/z"), 9, 3)));
        same("no list", "", ids(WidgetLogic.upNext(queue(), 2, 3)));
    }

    private static void up_next_has_only_rows_of_the_queue() {
        same("a row of another list", "queue/3/b", ids(WidgetLogic.upNext(queue("queue/2/a", "download/x", "queue/3/b"), 2, 3)));
    }

    private static void a_queue_row_is_told_to_the_player_and_is_not_there_without_one() {
        same("a song showing", COMMAND, WidgetLogic.tap(ROW, true));
        same("no song", HIDDEN, WidgetLogic.tap(ROW, false));
        same("left behind by a dead app", OPEN, WidgetLogic.coldTap(ROW));
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
