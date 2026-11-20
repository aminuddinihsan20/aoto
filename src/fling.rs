//! How far a page coasts when a finger lets it go, the way Android's own lists do.
//!
//! egui's coasting is iOS's: the speed dies away over one fixed time, so the distance
//! grows in step with the speed, and a gentle flick carries the page three times as far
//! as it does anywhere else on an Android phone. Android's (`OverScroller`) grows faster
//! than the speed: a gentle flick barely moves the page, a hard one sends it far.

/// Below this speed (points a second) letting go is not a fling: Android's minimum.
pub const MIN_SPEED: f32 = 50.0;

/// How far (points) a page let go at `speed` (points a second) coasts on Android.
pub fn distance(speed: f32) -> f32 {
    // OverScroller's own numbers, in dp: its friction (0.015) times gravity in inches
    // (386.09) times 160 dp an inch times 0.84, and its deceleration rate ln 0.78 / ln 0.9.
    const FRICTION: f32 = 0.015 * 386.0878 * 160.0 * 0.84;
    const RATE: f32 = 2.358_201_8;
    FRICTION * (0.35 * speed.abs() / FRICTION).powf(RATE / (RATE - 1.0))
}

/// The decay time that makes egui's coasting cover that distance: its distance is
/// speed times decay time.
pub fn decay_time(speed: f32) -> f32 {
    let speed = speed.abs();
    if speed < MIN_SPEED {
        return 0.01;
    }
    (distance(speed) / speed).clamp(0.01, 1.0)
}

/// A finger just let go: from now on the pages coast as far as Android would carry them
/// from that speed (every scroll area reads this one setting while it coasts).
pub fn on_release(ctx: &egui::Context) {
    let Some(v) = ctx.input(|i| i.pointer.any_released().then(|| i.pointer.velocity())) else { return };
    let time = decay_time(v.x.abs().max(v.y.abs()));
    ctx.all_styles_mut(|style| style.spacing.scroll.kinetic.decay_time = time);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame of a headless egui with these events at `time`.
    fn frame(ctx: &egui::Context, time: f64, events: Vec<egui::Event>) {
        let input = egui::RawInput { time: Some(time), events, ..Default::default() };
        ctx.run_ui(input, |_| on_release(ctx)).drop_without_applying_deltas();
    }

    fn coast_after_dragging(points_a_frame: f32) -> f32 {
        let ctx = egui::Context::default();
        let at = |y: f32| egui::pos2(200.0, y);
        let press = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(&ctx, 0.0, vec![egui::Event::PointerMoved(at(600.0)), press(at(600.0), true)]);
        let mut y = 600.0;
        for i in 1..=8 {
            y -= points_a_frame;
            frame(&ctx, i as f64 / 60.0, vec![egui::Event::PointerMoved(at(y))]);
        }
        frame(&ctx, 9.0 / 60.0, vec![press(at(y), false)]);
        ctx.global_style().spacing.scroll.kinetic.decay_time
    }

    #[test]
    fn letting_go_sets_how_long_the_pages_coast_by_the_fingers_speed() {
        let (gentle, hard) = (coast_after_dragging(8.0), coast_after_dragging(60.0));
        assert!(gentle < 0.2, "8 points a frame is 480 a second, a gentle flick: {gentle}");
        assert!(hard > 2.0 * gentle, "a hard flick coasts longer: {hard} against {gentle}");
        assert!(coast_after_dragging(0.0) <= 0.02, "a finger lifted at rest leaves the page where it is");
    }

    #[test]
    fn a_hard_flick_goes_about_as_far_as_androids_own() {
        // OverScroller for 8000 px/s on a 420 dpi screen: 3532 px, which is 1345 dp.
        let d = distance(3048.0);
        assert!((d - 1345.0).abs() < 15.0, "{d}");
    }

    #[test]
    fn a_gentle_flick_barely_moves_the_page() {
        // 2000 px/s on the same screen: 318 px, 121 dp. The old coasting went 381.
        let d = distance(762.0);
        assert!((d - 121.0).abs() < 5.0, "{d}");
        assert!(decay_time(762.0) < 0.2, "a third of the old half second: {}", decay_time(762.0));
    }

    #[test]
    fn the_distance_grows_faster_than_the_speed() {
        assert!(distance(2000.0) > 2.0 * distance(1000.0) * 1.5);
        assert_eq!(distance(-1500.0), distance(1500.0), "either way");
    }

    #[test]
    fn the_decay_time_times_the_speed_is_the_distance() {
        for v in [300.0, 762.0, 1500.0, 3048.0] {
            assert!((decay_time(v) * v - distance(v)).abs() < 1.0, "{v}");
        }
    }

    #[test]
    fn letting_go_slowly_is_no_fling_and_the_time_stays_in_bounds() {
        assert!(decay_time(20.0) <= 0.02, "the page stops where it was let go");
        assert!(decay_time(0.0) <= 0.02);
        assert!(decay_time(30_000.0) <= 1.0, "a wild speed does not coast for ever");
    }
}
