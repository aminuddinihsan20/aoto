package io.github.aminuddinihsan20.aoto;

/**
 * What the player does when another app takes the sound and gives it back (Android's
 * audio focus). Music that was playing when a call, the assistant or a voice note took the
 * sound for a moment comes back by itself; music the listener had paused never does.
 */
final class FocusLogic {
    /** AudioManager's AUDIOFOCUS_* values. */
    static final int GAIN = 1, LOSS = -1, LOSS_TRANSIENT = -2, LOSS_TRANSIENT_CAN_DUCK = -3;
    /** What to tell the player. */
    static final int NOTHING = 0, PAUSE = 1, PLAY = 2;

    private boolean held;
    /** The music was playing when the sound was taken for a moment, and is to play on. */
    private boolean resume;

    /** The system granted the request. */
    void granted() {
        held = true;
    }

    /** Must the sound be asked for (again) before playing? */
    boolean mustAsk() {
        return !held;
    }

    /** Is the music paused only until the sound comes back? */
    boolean resumes() {
        return resume;
    }

    /** The player's state as last seen: music that plays again has nothing to resume. */
    void playingNow(boolean playing) {
        if (playing) {
            resume = false;
        }
    }

    /** The listener paused, whatever the reason the music was silent. */
    void pausedByHand() {
        resume = false;
    }

    /** The system says the focus changed; `playing` is whether music plays right now. */
    int changed(int change, boolean playing) {
        switch (change) {
            case LOSS:
                held = false;
                resume = false;
                return playing ? PAUSE : NOTHING;
            case LOSS_TRANSIENT:
                if (playing) {
                    resume = true;
                    return PAUSE;
                }
                return NOTHING;
            case GAIN:
                held = true;
                if (resume) {
                    resume = false;
                    return playing ? NOTHING : PLAY;
                }
                return NOTHING;
            default:
                // Ducking: the system turns the music down and up itself.
                return NOTHING;
        }
    }
}
