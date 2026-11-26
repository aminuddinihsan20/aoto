//! The phone's bottom sheet: a panel that slides up over a dimmed screen, as tall as
//! what it holds (it doesn't scroll: that has to fit the screen), with a handle at its
//! top. A tap outside it, Android's Back and a swipe down close it. Whatever opens one
//! keeps a `bool` for it and calls `show` every frame; Back finds the open one through
//! `is_open` and `dismiss`. It draws in the mode (light or dark) set when it is shown.

use egui::{CornerRadius, Rect, Sense, Ui, pos2, vec2};

use crate::theme;

/// The room at the sheet's top with the handle in it, the margin under what it holds,
/// and the margin at its sides.
const HANDLE_H: f32 = 28.0;
const BOTTOM: f32 = 16.0;
const SIDE: f32 = 16.0;
/// Its top corners.
const RADIUS: u8 = 28;
/// The widest it gets, on a screen wider than a phone's.
const MAX_W: f32 = 560.0;

/// What a sheet remembers between frames.
#[derive(Clone, Copy, Default)]
struct State {
    /// How tall what it holds was when last laid out.
    content: f32,
    /// How far it is pulled down from its place.
    pull: f32,
}

/// The frame a sheet was last drawn open in, and the frame Back was last pressed in.
fn open_id() -> egui::Id {
    egui::Id::unique("sheet-open")
}

fn back_id() -> egui::Id {
    egui::Id::unique("sheet-back")
}

/// `id`'s frame number is this frame's or the one before: it still counts.
fn recent(ctx: &egui::Context, id: egui::Id) -> bool {
    ctx.data(|d| d.get_temp::<u64>(id)).is_some_and(|pass| pass + 1 >= ctx.cumulative_pass_nr())
}

/// Draw the sheet `key` while `open`, and while it slides away after: `add` lays out
/// what it holds, top down. `open` turns false when the sheet is dismissed. Returns
/// where the sheet is, when it is on screen.
pub fn show(ctx: &egui::Context, key: &str, open: &mut bool, add: impl FnOnce(&mut Ui)) -> Option<Rect> {
    let id = egui::Id::unique(("sheet", key));
    if *open && recent(ctx, back_id()) {
        ctx.data_mut(|d| d.remove::<u64>(back_id()));
        *open = false;
    }
    if *open {
        let pass = ctx.cumulative_pass_nr();
        ctx.data_mut(|d| d.insert_temp(open_id(), pass));
    }
    let t = theme::anim_bool(ctx, ("sheet-shown", key), *open, 0.3);
    if t <= 0.001 {
        ctx.data_mut(|d| d.remove::<State>(id));
        return None;
    }
    let mut st: State = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    let screen = ctx.content_rect();
    let insets = crate::platform::insets(ctx);
    let width = screen.width().min(MAX_W);
    let height = HANDLE_H + st.content + BOTTOM + insets.bottom;
    // Pulled down it keeps its distance from the finger, also while it slides away.
    let top = screen.bottom() - (height - st.pull).max(0.0) * t;
    let rect = Rect::from_min_size(pos2(screen.center().x - width / 2.0, top), vec2(width, height));
    egui::Area::new(id).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        // The dimmed screen takes every touch outside the sheet; a tap there closes it.
        let scrim = ui.interact(screen, id.with("scrim"), Sense::click_and_drag());
        ui.painter().rect_filled(screen, 0.0, theme::shade(0.5 * t));
        if scrim.clicked() {
            *open = false;
        }
        let body = ui.interact(rect, id.with("body"), Sense::click_and_drag());
        let corners = CornerRadius { nw: RADIUS, ne: RADIUS, sw: 0, se: 0 };
        ui.painter().rect_filled(rect, corners, theme::surface_fill());
        let handle = Rect::from_center_size(pos2(rect.center().x, rect.top() + 14.0), vec2(32.0, 4.0));
        if crate::material::on() {
            // Material's sheet is its surface alone, with the handle in the outline colour.
            ui.painter().rect_filled(handle, 2.0, crate::material::scheme().outline);
            // A lined look draws the sheet's edge (its foot is under the screen's).
            super::m3::outline(ui, rect.with_max_y(rect.bottom() + RADIUS as f32), corners);
        } else {
            ui.painter().rect_stroke(rect, corners, egui::Stroke::new(1.0, theme::glass(0.08)), egui::StrokeKind::Inside);
            ui.painter().rect_filled(handle, 2.0, theme::glass(0.32));
        }

        let held = body.dragged();
        if held {
            let pulled = ui.input(|i| i.pointer.press_origin().zip(i.pointer.interact_pos()).map(|(from, at)| at.y - from.y));
            st.pull = pulled.unwrap_or(0.0).max(0.0);
        } else if body.drag_stopped() && dismissed(st.pull, ui.input(|i| i.pointer.velocity().y), height) {
            *open = false;
        } else if *open && st.pull > 0.0 {
            // Let go short of closing: back up to its place.
            st.pull *= (-ui.input(|i| i.stable_dt).min(0.1) / 0.07).exp();
            if st.pull < 0.5 {
                st.pull = 0.0;
            }
            ui.ctx().request_repaint();
        }

        let inner = Rect::from_min_max(
            pos2(rect.left() + SIDE + insets.left, rect.top() + HANDLE_H),
            pos2(rect.right() - SIDE - insets.right, rect.top() + HANDLE_H + screen.height()),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
        child.set_clip_rect(rect.intersect(screen));
        add(&mut child);
        let content = child.min_rect().height();
        if content != st.content {
            // Its place depends on its height: draw again with the new one.
            ui.ctx().request_repaint();
        }
        st.content = content;
    });
    ctx.data_mut(|d| d.insert_temp(id, st));
    Some(rect)
}

/// Is a sheet open (as of the last frame)? Then Back closes it before anything else.
pub fn is_open(ctx: &egui::Context) -> bool {
    recent(ctx, open_id())
}

/// Close the open sheet: Android's Back.
pub fn dismiss(ctx: &egui::Context) {
    if is_open(ctx) {
        let pass = ctx.cumulative_pass_nr();
        ctx.data_mut(|d| d.insert_temp(back_id(), pass));
        ctx.request_repaint();
    }
}

/// Does letting go of the sheet `dy` points below its place, moving at `vy` points a
/// second (down is positive), send it away? A long pull or a quick flick down does.
fn dismissed(dy: f32, vy: f32, height: f32) -> bool {
    dy > 0.0 && ((dy > height * 0.25 && vy > -200.0) || vy > 600.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Pos2, Sense, pos2, vec2};

    /// A 400 x 800 phone in a headless egui with one sheet: 200 points of content with
    /// a button in its top half.
    struct Phone {
        ctx: egui::Context,
        open: bool,
        time: f64,
        /// Where the sheet was last drawn, and whether its button was pressed since.
        sheet: Option<Rect>,
        pressed: bool,
        /// The app stopped drawing the sheet (the screen it belongs to is gone).
        gone: bool,
    }

    impl Phone {
        fn new() -> Phone {
            let mut p = Phone { ctx: egui::Context::default(), open: true, time: 0.0, sheet: None, pressed: false, gone: false };
            // Slid all the way up before a finger comes down.
            p.wait(1.0);
            p
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 1.0 / 60.0;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 800.0))), time: Some(self.time), events, ..Default::default() };
            let (mut open, mut pressed, mut sheet, gone) = (self.open, false, None, self.gone);
            let out = self.ctx.run_ui(input, |ui| {
                if gone {
                    return;
                }
                sheet = show(ui.ctx(), "test", &mut open, |ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 100.0), Sense::hover());
                    pressed = ui.interact(r, ui.scope_id().with("button"), Sense::click()).clicked();
                    ui.allocate_exact_size(vec2(ui.available_width(), 100.0), Sense::hover());
                });
            });
            out.drop_without_applying_deltas();
            self.open = open;
            self.sheet = sheet;
            self.pressed |= pressed;
        }

        fn wait(&mut self, secs: f64) {
            for _ in 0..(secs * 60.0) as usize {
                self.frame(vec![]);
            }
        }

        fn down(&mut self, at: Pos2) {
            self.frame(vec![egui::Event::PointerMoved(at), egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
        }

        fn up(&mut self, at: Pos2) {
            self.frame(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
            self.frame(vec![]);
        }

        fn tap(&mut self, at: Pos2) {
            self.down(at);
            self.up(at);
        }

        /// A finger slides from `from` to `to` over `frames` frames, then lifts.
        fn swipe(&mut self, from: Pos2, to: Pos2, frames: usize) {
            self.down(from);
            for i in 1..=frames {
                self.frame(vec![egui::Event::PointerMoved(from + (to - from) * (i as f32 / frames as f32))]);
            }
            self.up(to);
        }

        fn top(&self) -> f32 {
            self.sheet.expect("the sheet is on screen").top()
        }
    }

    #[test]
    fn it_sits_at_the_bottom_as_tall_as_what_it_holds() {
        let p = Phone::new();
        let sheet = p.sheet.unwrap();
        assert_eq!(sheet.bottom(), 800.0);
        assert_eq!((sheet.left(), sheet.right()), (0.0, 400.0));
        // The 200 points it holds, the handle above and the margin below.
        assert!((230.0..270.0).contains(&sheet.height()), "{sheet:?}");
    }

    #[test]
    fn a_closed_sheet_is_not_drawn() {
        let mut p = Phone::new();
        p.open = false;
        p.wait(1.0);
        assert!(p.sheet.is_none());
        assert!(!is_open(&p.ctx));
    }

    #[test]
    fn a_tap_outside_closes_it() {
        let mut p = Phone::new();
        p.tap(pos2(200.0, 120.0));
        assert!(!p.open);
        // It slides away rather than vanishing.
        assert!(p.sheet.is_some());
        p.wait(1.0);
        assert!(p.sheet.is_none());
    }

    #[test]
    fn a_tap_on_the_sheet_leaves_it_open() {
        let mut p = Phone::new();
        let at = pos2(200.0, p.top() + 12.0);
        p.tap(at);
        p.tap(pos2(200.0, 790.0));
        assert!(p.open);
        assert!(!p.pressed);
    }

    #[test]
    fn what_it_holds_takes_its_own_taps() {
        let mut p = Phone::new();
        let at = pos2(200.0, p.top() + 60.0);
        p.tap(at);
        assert!(p.pressed);
        assert!(p.open);
    }

    #[test]
    fn a_swipe_down_closes_it() {
        let mut p = Phone::new();
        let from = pos2(200.0, p.top() + 12.0);
        p.swipe(from, from + vec2(0.0, 180.0), 12);
        assert!(!p.open);
    }

    #[test]
    fn a_swipe_down_that_starts_on_a_button_closes_it_without_pressing_it() {
        let mut p = Phone::new();
        let from = pos2(200.0, p.top() + 60.0);
        p.swipe(from, from + vec2(0.0, 180.0), 12);
        assert!(!p.open);
        assert!(!p.pressed);
    }

    #[test]
    fn it_follows_the_finger_down_and_not_up() {
        let mut p = Phone::new();
        let rest = p.top();
        let from = pos2(200.0, rest + 12.0);
        p.down(from);
        p.frame(vec![egui::Event::PointerMoved(from + vec2(0.0, 40.0))]);
        p.frame(vec![]);
        assert!((p.top() - (rest + 40.0)).abs() < 1.0, "{} from {rest}", p.top());
        p.frame(vec![egui::Event::PointerMoved(from + vec2(0.0, -60.0))]);
        p.frame(vec![]);
        assert_eq!(p.top(), rest);
    }

    #[test]
    fn a_short_slow_pull_springs_back() {
        let mut p = Phone::new();
        let rest = p.top();
        let from = pos2(200.0, rest + 12.0);
        p.swipe(from, from + vec2(0.0, 30.0), 40);
        assert!(p.open);
        p.wait(1.0);
        assert!((p.top() - rest).abs() < 0.5);
    }

    #[test]
    fn back_closes_the_open_sheet() {
        let mut p = Phone::new();
        assert!(is_open(&p.ctx));
        dismiss(&p.ctx);
        p.frame(vec![]);
        assert!(!p.open);
        p.frame(vec![]);
        assert!(!is_open(&p.ctx));
        // A Back with no sheet open is not kept for the next one.
        dismiss(&p.ctx);
        p.wait(1.0);
        p.open = true;
        p.wait(1.0);
        assert!(p.open);
    }

    #[test]
    fn a_sheet_no_longer_drawn_does_not_hold_back() {
        let mut p = Phone::new();
        p.gone = true;
        p.frame(vec![]);
        p.frame(vec![]);
        assert!(!is_open(&p.ctx));
    }

    #[test]
    fn a_long_pull_or_a_quick_flick_down_sends_it_away() {
        assert!(dismissed(120.0, 0.0, 400.0), "pulled a good part of the way");
        assert!(dismissed(20.0, 900.0, 400.0), "flicked down");
        assert!(!dismissed(30.0, 100.0, 400.0), "a nudge");
        assert!(!dismissed(0.0, 0.0, 400.0));
        assert!(!dismissed(150.0, -900.0, 400.0), "thrown back up");
        assert!(dismissed(150.0, -20.0, 400.0), "a finger is never quite still");
    }
}
