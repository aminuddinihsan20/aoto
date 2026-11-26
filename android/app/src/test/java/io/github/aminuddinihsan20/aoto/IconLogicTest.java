package io.github.aminuddinihsan20.aoto;

import static io.github.aminuddinihsan20.aoto.IconLogic.STATE_DEFAULT;
import static io.github.aminuddinihsan20.aoto.IconLogic.STATE_DISABLED;
import static io.github.aminuddinihsan20.aoto.IconLogic.STATE_ENABLED;

/** Which home-screen entry shows for a look, checked with nothing but a JDK: `sh android/test-java.sh`. */
public final class IconLogicTest {
    private static int failures;

    public static void main(String[] args) {
        Runnable[] tests = {
            IconLogicTest::each_look_has_its_own_entry,
            IconLogicTest::a_look_without_an_entry_shows_the_first,
            IconLogicTest::a_fresh_install_shows_the_first_look_only,
            IconLogicTest::choosing_sticker_enables_its_entry_and_disables_the_other,
            IconLogicTest::going_back_to_indigo_swaps_them_again,
            IconLogicTest::nothing_is_touched_when_the_icon_already_shows,
            IconLogicTest::exactly_one_entry_shows_whatever_was_chosen,
        };
        for (Runnable t : tests) {
            t.run();
        }
        if (failures > 0) {
            System.out.println(failures + " FAILED");
            System.exit(1);
        }
        System.out.println(tests.length + " passed");
    }

    private static void check(boolean ok, String what) {
        if (!ok) {
            failures++;
            System.out.println("FAILED: " + what);
        }
    }

    static void each_look_has_its_own_entry() {
        check(".LauncherIndigo".equals(IconLogic.alias("indigo")), "indigo's entry");
        check(".LauncherSticker".equals(IconLogic.alias("sticker")), "sticker's entry");
        check(".LauncherMidnight".equals(IconLogic.alias("midnight")), "midnight's entry");
    }

    static void a_look_without_an_entry_shows_the_first() {
        check(".LauncherIndigo".equals(IconLogic.alias("neon")), "an unknown look");
        check(".LauncherIndigo".equals(IconLogic.alias("")), "no look");
        check(".LauncherIndigo".equals(IconLogic.alias(null)), "null");
    }

    static void a_fresh_install_shows_the_first_look_only() {
        check(IconLogic.enabled("indigo", STATE_DEFAULT), "indigo by default");
        check(!IconLogic.enabled("sticker", STATE_DEFAULT), "sticker not by default");
    }

    static void choosing_sticker_enables_its_entry_and_disables_the_other() {
        check(IconLogic.change("sticker", STATE_DEFAULT, "sticker") == STATE_ENABLED, "sticker enabled");
        check(IconLogic.change("indigo", STATE_DEFAULT, "sticker") == STATE_DISABLED, "indigo disabled");
    }

    static void going_back_to_indigo_swaps_them_again() {
        check(IconLogic.change("indigo", STATE_DISABLED, "indigo") == STATE_ENABLED, "indigo enabled");
        check(IconLogic.change("sticker", STATE_ENABLED, "indigo") == STATE_DISABLED, "sticker disabled");
    }

    static void nothing_is_touched_when_the_icon_already_shows() {
        check(IconLogic.change("indigo", STATE_DEFAULT, "indigo") == -1, "indigo left");
        check(IconLogic.change("sticker", STATE_DEFAULT, "indigo") == -1, "sticker left");
        check(IconLogic.change("sticker", STATE_ENABLED, "sticker") == -1, "sticker left, chosen");
        check(IconLogic.change("indigo", STATE_DISABLED, "sticker") == -1, "indigo left, not chosen");
    }

    static void exactly_one_entry_shows_whatever_was_chosen() {
        int[] states = {STATE_DEFAULT, STATE_ENABLED, STATE_DISABLED};
        for (String wanted : new String[] {"indigo", "sticker", "neon"}) {
            for (int indigo : states) {
                for (int sticker : states) {
                    int shown = 0;
                    for (String look : IconLogic.ICONS) {
                        int state = "indigo".equals(look) ? indigo : "sticker".equals(look) ? sticker : STATE_DEFAULT;
                        int change = IconLogic.change(look, state, wanted);
                        if (IconLogic.enabled(look, change == -1 ? state : change)) {
                            shown++;
                        }
                    }
                    check(shown == 1, wanted + " from " + indigo + "/" + sticker + " shows " + shown);
                }
            }
        }
    }
}
