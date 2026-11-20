package io.github.aminuddinihsan20.aoto;

/**
 * What "Play on" decides, apart from Android so it can be checked on a desktop
 * (`sh android/test-java.sh`): which of the connected devices the music plays through,
 * what it is called, how the output switcher is opened on each Android, and the media
 * volume's steps. Device types are AudioDeviceInfo's TYPE_ numbers.
 */
final class OutputLogic {
    /** The kinds of output the app names (src/playon.rs counts them the same). */
    static final int PHONE = 0, BLUETOOTH = 1, WIRED = 2, USB = 3, EXTERNAL = 4, HEARING_AID = 5, OTHER = 6;

    /** The ways to let the listener pick another output, see {@link #switchers}. */
    static final int ROUTER = 1, SYSTEM_UI = 2, SETTINGS_PANEL = 3, BLUETOOTH_SETTINGS = 4;

    /** What the app says its button opens: nothing, Android's output switcher, or the Bluetooth settings. */
    static final int OFFER_NOTHING = 0, OFFER_SWITCHER = 1, OFFER_BLUETOOTH = 2;

    private OutputLogic() {
    }

    /** What kind of output a device type is. */
    static int kind(int deviceType) {
        switch (deviceType) {
            case 1: // the earpiece
            case 2: // the speaker
            case 18: // telephony
            case 24: // the speaker, held to a safe loudness
                return PHONE;
            case 7: // Bluetooth SCO
            case 8: // Bluetooth A2DP
            case 26: // LE Audio headset
            case 27: // LE Audio speaker
            case 30: // LE Audio broadcast
                return BLUETOOTH;
            case 3: // wired headset
            case 4: // wired headphones
            case 5: // analog line
            case 6: // digital line
            case 19: // aux line
                return WIRED;
            case 11: // USB device
            case 12: // USB accessory
            case 22: // USB headset
                return USB;
            case 9: // HDMI
            case 10: // HDMI ARC
            case 13: // dock
            case 29: // HDMI eARC
            case 31: // analog dock
                return EXTERNAL;
            case 23:
                return HEARING_AID;
            default:
                return OTHER;
        }
    }

    /**
     * How strongly Android (before 13, which says so itself) prefers a device for music:
     * a hearing aid, then Bluetooth, a wire, USB, a dock or screen, and the speaker last.
     * 0: not a device music plays through (a call's devices, a recording's).
     */
    private static int rank(int deviceType) {
        switch (deviceType) {
            case 23:
                return 100;
            case 8:
            case 26:
            case 27:
            case 30:
                return 90;
            case 4:
                return 80;
            case 5:
                return 79;
            case 6:
                return 78;
            case 3:
                return 77;
            case 19:
                return 76;
            case 22:
                return 70;
            case 12:
                return 69;
            case 11:
                return 68;
            case 13:
                return 60;
            case 31:
                return 59;
            case 9:
                return 58;
            case 10:
                return 57;
            case 29:
                return 56;
            case 21: // a car's audio bus
                return 20;
            case 20: // a network device
                return 19;
            case 32: // a speaker group
                return 18;
            case 2:
                return 10;
            case 24:
                return 9;
            default:
                return 0;
        }
    }

    /** Which of the connected output devices music plays through: its place in the list, or -1. */
    static int pick(int[] deviceTypes) {
        int best = -1, bestRank = 0;
        for (int i = 0; i < deviceTypes.length; i++) {
            int rank = rank(deviceTypes[i]);
            if (rank > bestRank) {
                best = i;
                bestRank = rank;
            }
        }
        return best;
    }

    /**
     * The name to show for a device: its own. The phone's speaker has none to show, and
     * Android names a device that has none of its own after the phone's model.
     */
    static String name(int kind, String product, String model) {
        String name = product == null ? "" : product.trim();
        return kind == PHONE || name.equals(model) ? "" : name;
    }

    /**
     * The ways to let the listener pick another output on this Android, in the order to
     * try them. Android 14 has a call for its output switcher (MediaRouter2). On 12 and
     * 13 the System UI shows it when asked by a broadcast, on 10 and 11 the Settings app
     * shows it as a panel; neither is a public constant, they are the ones
     * androidx.mediarouter uses, and each is only used if a system app answers it. Before
     * that, and whenever those fail, there are the Bluetooth settings.
     */
    static int[] switchers(int sdk) {
        if (sdk >= 34) {
            return new int[] {ROUTER, BLUETOOTH_SETTINGS};
        }
        if (sdk >= 31) {
            return new int[] {SYSTEM_UI, SETTINGS_PANEL, BLUETOOTH_SETTINGS};
        }
        if (sdk >= 29) {
            return new int[] {SETTINGS_PANEL, BLUETOOTH_SETTINGS};
        }
        return new int[] {BLUETOOTH_SETTINGS};
    }

    /** What the app tells the listener its button opens, for the first way that is there (0: none is). */
    static int offer(int way) {
        if (way == BLUETOOTH_SETTINGS) {
            return OFFER_BLUETOOTH;
        }
        return way == ROUTER || way == SYSTEM_UI || way == SETTINGS_PANEL ? OFFER_SWITCHER : OFFER_NOTHING;
    }

    /**
     * The media volume as the app's slider counts it: the step it is on and how many
     * there are, from the stream's least. No steps when the volume can't be set.
     */
    static int[] steps(int volume, int min, int max, boolean fixed) {
        if (fixed || max <= min) {
            return new int[] {0, 0};
        }
        return new int[] {Math.max(0, Math.min(volume, max) - min), max - min};
    }

    /** The stream's volume for a step of the app's slider. */
    static int index(int step, int min, int max) {
        return (int) Math.max(min, Math.min((long) step + min, max));
    }

    /** The report the app reads (Output::from_line in src/playon.rs): fields joined by U+001F. */
    static String line(int kind, String name, int volume, int max, int offer) {
        String clean = name.replace('\u001f', ' ').replace('\n', ' ').replace('\r', ' ');
        return kind + "\u001f" + clean + "\u001f" + volume + "\u001f" + max + "\u001f" + offer;
    }
}
