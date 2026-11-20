package io.github.aminuddinihsan20.bitchord;

import static io.github.aminuddinihsan20.bitchord.OutputLogic.BLUETOOTH;
import static io.github.aminuddinihsan20.bitchord.OutputLogic.BLUETOOTH_SETTINGS;
import static io.github.aminuddinihsan20.bitchord.OutputLogic.EXTERNAL;
import static io.github.aminuddinihsan20.bitchord.OutputLogic.HEARING_AID;
import static io.github.aminuddinihsan20.bitchord.OutputLogic.OFFER_BLUETOOTH;
import static io.github.aminuddinihsan20.bitchord.OutputLogic.OFFER_NOTHING;
import static io.github.aminuddinihsan20.bitchord.OutputLogic.OFFER_SWITCHER;
import static io.github.aminuddinihsan20.bitchord.OutputLogic.OTHER;
import static io.github.aminuddinihsan20.bitchord.OutputLogic.PHONE;
import static io.github.aminuddinihsan20.bitchord.OutputLogic.ROUTER;
import static io.github.aminuddinihsan20.bitchord.OutputLogic.SETTINGS_PANEL;
import static io.github.aminuddinihsan20.bitchord.OutputLogic.SYSTEM_UI;
import static io.github.aminuddinihsan20.bitchord.OutputLogic.USB;
import static io.github.aminuddinihsan20.bitchord.OutputLogic.WIRED;

import java.util.Arrays;

/**
 * What "Play on" decides, checked with nothing but a JDK: `sh android/test-java.sh`. The
 * device types are AudioDeviceInfo's, by their values.
 */
public final class OutputLogicTest {
    private static final int EARPIECE = 1, SPEAKER = 2, WIRED_HEADSET = 3, WIRED_HEADPHONES = 4, LINE_ANALOG = 5, SCO = 7, A2DP = 8, HDMI = 9,
            USB_DEVICE = 11, DOCK = 13, TELEPHONY = 18, BUS = 21, USB_HEADSET = 22, HEARING_AID_TYPE = 23, REMOTE_SUBMIX = 25, BLE_HEADSET = 26, BLE_SPEAKER = 27;

    private static int failures;

    public static void main(String[] args) {
        Runnable[] tests = {
            OutputLogicTest::every_device_type_is_one_of_the_kinds_the_app_names,
            OutputLogicTest::bluetooth_wins_over_a_wire_and_a_wire_over_the_speaker,
            OutputLogicTest::a_hearing_aid_wins_over_everything,
            OutputLogicTest::what_is_not_for_music_is_never_picked,
            OutputLogicTest::with_nothing_to_play_through_nothing_is_picked,
            OutputLogicTest::a_device_keeps_its_own_name,
            OutputLogicTest::the_phones_own_model_is_not_a_devices_name,
            OutputLogicTest::android_14_opens_its_output_switcher_itself,
            OutputLogicTest::android_12_and_13_ask_the_system_ui_then_the_settings_panel,
            OutputLogicTest::android_10_and_11_have_the_settings_panel,
            OutputLogicTest::older_androids_only_have_bluetooth_settings,
            OutputLogicTest::every_android_ends_at_bluetooth_settings,
            OutputLogicTest::the_listener_is_told_what_the_button_opens,
            OutputLogicTest::the_volume_is_counted_from_its_least,
            OutputLogicTest::a_fixed_volume_has_no_steps,
            OutputLogicTest::a_volume_asked_for_stays_within_the_streams_range,
            OutputLogicTest::the_report_is_one_line_a_name_cannot_break,
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

    private static void every_device_type_is_one_of_the_kinds_the_app_names() {
        same("speaker", PHONE, OutputLogic.kind(SPEAKER));
        same("earpiece", PHONE, OutputLogic.kind(EARPIECE));
        same("a2dp", BLUETOOTH, OutputLogic.kind(A2DP));
        same("le headset", BLUETOOTH, OutputLogic.kind(BLE_HEADSET));
        same("le speaker", BLUETOOTH, OutputLogic.kind(BLE_SPEAKER));
        same("headphones", WIRED, OutputLogic.kind(WIRED_HEADPHONES));
        same("headset", WIRED, OutputLogic.kind(WIRED_HEADSET));
        same("line", WIRED, OutputLogic.kind(LINE_ANALOG));
        same("usb headset", USB, OutputLogic.kind(USB_HEADSET));
        same("usb device", USB, OutputLogic.kind(USB_DEVICE));
        same("hdmi", EXTERNAL, OutputLogic.kind(HDMI));
        same("dock", EXTERNAL, OutputLogic.kind(DOCK));
        same("hearing aid", HEARING_AID, OutputLogic.kind(HEARING_AID_TYPE));
        same("a car's bus", OTHER, OutputLogic.kind(BUS));
        same("a type from a later Android", OTHER, OutputLogic.kind(99));
    }

    private static void bluetooth_wins_over_a_wire_and_a_wire_over_the_speaker() {
        same("speaker alone", 0, OutputLogic.pick(new int[] {SPEAKER}));
        same("headphones plugged in", 2, OutputLogic.pick(new int[] {EARPIECE, SPEAKER, WIRED_HEADPHONES}));
        same("bluetooth and a wire", 3, OutputLogic.pick(new int[] {EARPIECE, SPEAKER, WIRED_HEADPHONES, A2DP}));
        same("bluetooth listed first", 0, OutputLogic.pick(new int[] {A2DP, SPEAKER, USB_HEADSET}));
        same("usb over hdmi", 1, OutputLogic.pick(new int[] {HDMI, USB_HEADSET, SPEAKER}));
    }

    private static void a_hearing_aid_wins_over_everything() {
        same("hearing aid", 1, OutputLogic.pick(new int[] {A2DP, HEARING_AID_TYPE, SPEAKER}));
    }

    private static void what_is_not_for_music_is_never_picked() {
        same("a call's devices", 2, OutputLogic.pick(new int[] {EARPIECE, TELEPHONY, SPEAKER, SCO, REMOTE_SUBMIX}));
    }

    private static void with_nothing_to_play_through_nothing_is_picked() {
        same("none", -1, OutputLogic.pick(new int[] {}));
        same("only a call's", -1, OutputLogic.pick(new int[] {EARPIECE, TELEPHONY}));
    }

    private static void a_device_keeps_its_own_name() {
        same("bluetooth", "Pixel Buds Pro", OutputLogic.name(BLUETOOTH, "Pixel Buds Pro", "Pixel 8"));
        same("usb", "USB-C to 3.5mm Adapter", OutputLogic.name(USB, " USB-C to 3.5mm Adapter ", "Pixel 8"));
    }

    private static void the_phones_own_model_is_not_a_devices_name() {
        // Android names a device that has no name of its own after the phone.
        same("wired", "", OutputLogic.name(WIRED, "Pixel 8", "Pixel 8"));
        same("bluetooth without a name", "", OutputLogic.name(BLUETOOTH, "Pixel 8", "Pixel 8"));
        same("the phone", "", OutputLogic.name(PHONE, "Pixel 8", "Pixel 8"));
        same("the phone under another name", "", OutputLogic.name(PHONE, "speaker", "Pixel 8"));
        same("no name", "", OutputLogic.name(BLUETOOTH, null, "Pixel 8"));
        same("an empty name", "", OutputLogic.name(BLUETOOTH, "  ", "Pixel 8"));
    }

    private static void android_14_opens_its_output_switcher_itself() {
        same("14", new int[] {ROUTER, BLUETOOTH_SETTINGS}, OutputLogic.switchers(34));
        same("16", new int[] {ROUTER, BLUETOOTH_SETTINGS}, OutputLogic.switchers(36));
    }

    private static void android_12_and_13_ask_the_system_ui_then_the_settings_panel() {
        same("12", new int[] {SYSTEM_UI, SETTINGS_PANEL, BLUETOOTH_SETTINGS}, OutputLogic.switchers(31));
        same("13", new int[] {SYSTEM_UI, SETTINGS_PANEL, BLUETOOTH_SETTINGS}, OutputLogic.switchers(33));
    }

    private static void android_10_and_11_have_the_settings_panel() {
        same("10", new int[] {SETTINGS_PANEL, BLUETOOTH_SETTINGS}, OutputLogic.switchers(29));
        same("11", new int[] {SETTINGS_PANEL, BLUETOOTH_SETTINGS}, OutputLogic.switchers(30));
    }

    private static void older_androids_only_have_bluetooth_settings() {
        same("8", new int[] {BLUETOOTH_SETTINGS}, OutputLogic.switchers(26));
        same("9", new int[] {BLUETOOTH_SETTINGS}, OutputLogic.switchers(28));
    }

    private static void every_android_ends_at_bluetooth_settings() {
        for (int sdk = 26; sdk <= 40; sdk++) {
            int[] ways = OutputLogic.switchers(sdk);
            same("sdk " + sdk, BLUETOOTH_SETTINGS, ways[ways.length - 1]);
        }
    }

    private static void the_listener_is_told_what_the_button_opens() {
        same("the router's", OFFER_SWITCHER, OutputLogic.offer(ROUTER));
        same("the system ui's", OFFER_SWITCHER, OutputLogic.offer(SYSTEM_UI));
        same("the panel", OFFER_SWITCHER, OutputLogic.offer(SETTINGS_PANEL));
        same("bluetooth settings", OFFER_BLUETOOTH, OutputLogic.offer(BLUETOOTH_SETTINGS));
        same("nothing opens", OFFER_NOTHING, OutputLogic.offer(0));
    }

    private static void the_volume_is_counted_from_its_least() {
        same("from zero", new int[] {7, 15}, OutputLogic.steps(7, 0, 15, false));
        same("a stream that starts at one", new int[] {4, 14}, OutputLogic.steps(5, 1, 15, false));
        same("below its least", new int[] {0, 14}, OutputLogic.steps(0, 1, 15, false));
    }

    private static void a_fixed_volume_has_no_steps() {
        same("fixed", new int[] {0, 0}, OutputLogic.steps(15, 0, 15, true));
        same("no range", new int[] {0, 0}, OutputLogic.steps(3, 3, 3, false));
    }

    private static void a_volume_asked_for_stays_within_the_streams_range() {
        same("a step", 8, OutputLogic.index(7, 1, 15));
        same("past the top", 15, OutputLogic.index(40, 1, 15));
        same("below zero", 1, OutputLogic.index(-3, 1, 15));
    }

    private static void the_report_is_one_line_a_name_cannot_break() {
        same("bluetooth", "1\u001fPixel Buds Pro\u001f7\u001f15\u001f1", OutputLogic.line(BLUETOOTH, "Pixel Buds Pro", 7, 15, OFFER_SWITCHER));
        same("a name with the separator", "1\u001fa b c\u001f0\u001f0\u001f2", OutputLogic.line(BLUETOOTH, "a\u001fb\nc", 0, 0, OFFER_BLUETOOTH));
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
