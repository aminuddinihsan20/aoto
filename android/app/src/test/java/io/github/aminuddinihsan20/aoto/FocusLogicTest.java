package io.github.aminuddinihsan20.aoto;

import static io.github.aminuddinihsan20.aoto.FocusLogic.GAIN;
import static io.github.aminuddinihsan20.aoto.FocusLogic.LOSS;
import static io.github.aminuddinihsan20.aoto.FocusLogic.LOSS_TRANSIENT;
import static io.github.aminuddinihsan20.aoto.FocusLogic.LOSS_TRANSIENT_CAN_DUCK;
import static io.github.aminuddinihsan20.aoto.FocusLogic.NOTHING;
import static io.github.aminuddinihsan20.aoto.FocusLogic.PAUSE;
import static io.github.aminuddinihsan20.aoto.FocusLogic.PLAY;

/**
 * What the player does when another app takes the sound and gives it back, checked with
 * nothing but a JDK: `sh android/test-java.sh`.
 */
public final class FocusLogicTest {
    private static int failures;

    public static void main(String[] args) {
        Runnable[] tests = {
            FocusLogicTest::music_interrupted_for_a_moment_comes_back_by_itself,
            FocusLogicTest::music_the_listener_paused_stays_paused_when_another_app_borrows_the_sound,
            FocusLogicTest::a_pause_by_hand_during_the_interruption_is_kept,
            FocusLogicTest::play_by_hand_during_the_interruption_is_not_played_again,
            FocusLogicTest::another_player_taking_over_for_good_pauses_and_nothing_comes_back,
            FocusLogicTest::a_sound_that_only_asks_for_quiet_changes_nothing,
            FocusLogicTest::after_losing_the_sound_for_good_it_is_asked_for_again,
        };
        for (Runnable t : tests) {
            t.run();
        }
        System.out.println((tests.length - failures) + " passed, " + failures + " failed");
        if (failures > 0) {
            System.exit(1);
        }
    }

    private static void same(int got, int want, String what) {
        if (got != want) {
            failures++;
            System.out.println("FAILED " + what + ": got " + got + ", want " + want);
        }
    }

    private static void yes(boolean got, String what) {
        if (!got) {
            failures++;
            System.out.println("FAILED " + what);
        }
    }

    static void music_interrupted_for_a_moment_comes_back_by_itself() {
        FocusLogic f = new FocusLogic();
        f.granted();
        same(f.changed(LOSS_TRANSIENT, true), PAUSE, "a call or the assistant pauses playing music");
        f.playingNow(false);
        yes(f.resumes(), "it is waiting to come back");
        same(f.changed(GAIN, false), PLAY, "the sound is back: play on");
        yes(!f.resumes(), "and that is done");
    }

    static void music_the_listener_paused_stays_paused_when_another_app_borrows_the_sound() {
        FocusLogic f = new FocusLogic();
        f.granted();
        f.playingNow(true);
        f.playingNow(false); // the listener pauses
        same(f.changed(LOSS_TRANSIENT, false), NOTHING, "nothing plays: nothing to pause");
        same(f.changed(GAIN, false), NOTHING, "it was paused by hand: it stays paused");
    }

    static void a_pause_by_hand_during_the_interruption_is_kept() {
        FocusLogic f = new FocusLogic();
        f.granted();
        same(f.changed(LOSS_TRANSIENT, true), PAUSE, "interrupted");
        f.playingNow(false);
        f.pausedByHand();
        same(f.changed(GAIN, false), NOTHING, "the listener said pause meanwhile");
    }

    static void play_by_hand_during_the_interruption_is_not_played_again() {
        FocusLogic f = new FocusLogic();
        f.granted();
        same(f.changed(LOSS_TRANSIENT, true), PAUSE, "interrupted");
        f.playingNow(false);
        f.playingNow(true); // started again by hand
        same(f.changed(GAIN, true), NOTHING, "already playing");
        yes(!f.resumes(), "nothing left to bring back");
    }

    static void another_player_taking_over_for_good_pauses_and_nothing_comes_back() {
        FocusLogic f = new FocusLogic();
        f.granted();
        same(f.changed(LOSS, true), PAUSE, "another music app plays");
        f.playingNow(false);
        same(f.changed(GAIN, false), NOTHING, "never by itself after that");
        FocusLogic g = new FocusLogic();
        g.granted();
        same(g.changed(LOSS_TRANSIENT, true), PAUSE, "interrupted");
        same(g.changed(LOSS, false), NOTHING, "then lost for good while paused");
        same(g.changed(GAIN, false), NOTHING, "and it does not come back");
    }

    static void a_sound_that_only_asks_for_quiet_changes_nothing() {
        FocusLogic f = new FocusLogic();
        f.granted();
        same(f.changed(LOSS_TRANSIENT_CAN_DUCK, true), NOTHING, "the system turns the music down itself");
        same(f.changed(GAIN, true), NOTHING, "and up again");
    }

    static void after_losing_the_sound_for_good_it_is_asked_for_again() {
        FocusLogic f = new FocusLogic();
        yes(f.mustAsk(), "at first");
        f.granted();
        yes(!f.mustAsk(), "held");
        f.changed(LOSS_TRANSIENT, true);
        yes(!f.mustAsk(), "a moment's loss keeps the request");
        f.changed(LOSS, false);
        yes(f.mustAsk(), "lost for good: the next play asks again");
    }
}
