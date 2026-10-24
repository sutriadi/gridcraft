//! Turns a finger drag into scrolling.
//!
//! egui-winit emulates a mouse from the first finger, so a drag on the document would select text
//! and the page would never scroll. This filter sits on the raw input and, for a one-finger touch:
//!
//! * a tap (no real movement) becomes a click at the touch point, delivered when the finger lifts;
//! * a drag becomes `MouseWheel` events (point units, with phases) that the canvas already knows
//!   how to scroll and coast with, and the emulated button press is withheld;
//! * a long press (finger held still) starts a mouse-style press so text can still be selected;
//! * a touch that starts at the right or bottom edge is left alone, so the scroll bars can be dragged.

#[cfg(test)]
use egui::pos2;
use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, TouchId, TouchPhase, Vec2};

/// Movement (points) before a touch counts as a drag instead of a tap.
const SLOP: f32 = 10.0;
/// How long (seconds) a still finger must rest before it selects instead of scrolls.
const LONG_PRESS: f64 = 0.45;
/// Width (points) of the screen edge strips where touches pass straight through (scroll bars).
const EDGE: f32 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    /// Finger is down, undecided between tap, scroll and select.
    Pending,
    Scrolling,
    /// Long press or edge touch: the emulated mouse events pass through untouched.
    Passthrough,
}

#[derive(Clone, Copy, Debug)]
struct Active {
    id: TouchId,
    start: Pos2,
    last: Pos2,
    began: f64,
    mode: Mode,
}

/// Stateful touch filter. Feed it every frame's raw input.
#[derive(Default)]
pub struct TouchScroll {
    active: Option<Active>,
    /// The emulated release that follows a withheld press must be dropped too.
    withhold_release: bool,
}

impl TouchScroll {
    /// True while a finger is down and undecided (the host should keep repainting to catch a long press).
    pub fn is_pending(&self) -> bool {
        matches!(self.active, Some(Active { mode: Mode::Pending, .. }))
    }

    /// Rewrites `raw.events` in place. `now` is the frame time in seconds.
    pub fn filter(&mut self, raw: &mut RawInput, now: f64) {
        let screen = raw.screen_rect.unwrap_or(egui::Rect::EVERYTHING);
        let events = std::mem::take(&mut raw.events);
        let mut out = Vec::with_capacity(events.len() + 2);
        for ev in events {
            match ev {
                Event::Touch { id, phase, pos, .. } => {
                    self.on_touch(id, phase, pos, now, screen, &mut out);
                    out.push(ev);
                }
                Event::PointerButton { button: PointerButton::Primary, pressed, .. }
                    if self.active.is_some_and(|a| a.mode != Mode::Passthrough) || (!pressed && self.withhold_release) =>
                {
                    // Withheld: a tap re-creates it on release, a long press on the timer.
                    if !pressed {
                        self.withhold_release = false;
                    }
                }
                other => out.push(other),
            }
        }
        // A finger resting long enough starts selecting.
        if let Some(a) = &mut self.active
            && a.mode == Mode::Pending
            && now - a.began >= LONG_PRESS
        {
            a.mode = Mode::Passthrough;
            let p = a.start;
            out.push(Event::PointerMoved(p));
            out.push(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        }
        raw.events = out;
    }

    fn on_touch(&mut self, id: TouchId, phase: TouchPhase, pos: Pos2, now: f64, screen: egui::Rect, out: &mut Vec<Event>) {
        match phase {
            TouchPhase::Start => {
                if self.active.is_some() {
                    return; // a second finger: pinch and friends are egui's business
                }
                let edge = pos.x >= screen.right() - EDGE || pos.y >= screen.bottom() - EDGE;
                let mode = if edge { Mode::Passthrough } else { Mode::Pending };
                self.active = Some(Active { id, start: pos, last: pos, began: now, mode });
                if !edge {
                    out.push(Event::PointerMoved(pos)); // hover target for the wheel
                }
            }
            TouchPhase::Move => {
                let Some(a) = &mut self.active else { return };
                if a.id != id {
                    return;
                }
                let step = pos - a.last;
                a.last = pos;
                match a.mode {
                    Mode::Pending => {
                        if (pos - a.start).length() >= SLOP {
                            a.mode = Mode::Scrolling;
                            // The slop distance itself is delivered, so the page follows the finger from the start.
                            out.push(wheel(pos - a.start, TouchPhase::Start));
                        }
                    }
                    Mode::Scrolling => out.push(wheel(step, TouchPhase::Move)),
                    Mode::Passthrough => {}
                }
            }
            TouchPhase::End | TouchPhase::Cancel => {
                let Some(a) = self.active else { return };
                if a.id != id {
                    return;
                }
                self.active = None;
                self.withhold_release = a.mode != Mode::Passthrough;
                match a.mode {
                    Mode::Pending if phase == TouchPhase::End => {
                        out.push(Event::PointerMoved(pos));
                        out.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
                        out.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
                    }
                    Mode::Scrolling => out.push(wheel(Vec2::ZERO, phase_end(phase))),
                    _ => {}
                }
            }
        }
    }
}

fn phase_end(phase: TouchPhase) -> TouchPhase {
    if phase == TouchPhase::Cancel { TouchPhase::Cancel } else { TouchPhase::End }
}

fn wheel(delta: Vec2, phase: TouchPhase) -> Event {
    Event::MouseWheel { unit: MouseWheelUnit::Point, delta, phase, modifiers: Modifiers::NONE }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(id: u64, phase: TouchPhase, p: Pos2) -> Event {
        Event::Touch { device_id: egui::TouchDeviceId(0), id: TouchId(id), phase, pos: p, force: None }
    }
    fn press(p: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }
    fn frame(f: &mut TouchScroll, events: Vec<Event>, now: f64) -> Vec<Event> {
        let mut raw = RawInput { screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 800.0))), events, ..Default::default() };
        f.filter(&mut raw, now);
        raw.events
    }
    fn wheels(ev: &[Event]) -> Vec<(Vec2, TouchPhase)> {
        ev.iter().filter_map(|e| if let Event::MouseWheel { delta, phase, .. } = e { Some((*delta, *phase)) } else { None }).collect()
    }
    fn presses(ev: &[Event]) -> usize {
        ev.iter().filter(|e| matches!(e, Event::PointerButton { .. })).count()
    }

    #[test]
    fn tap_becomes_click_on_release() {
        let mut f = TouchScroll::default();
        let p = pos2(100.0, 100.0);
        let a = frame(&mut f, vec![touch(1, TouchPhase::Start, p), press(p, true)], 0.0);
        assert_eq!(presses(&a), 0, "the press waits for the release");
        let b = frame(&mut f, vec![touch(1, TouchPhase::End, p), press(p, false)], 0.1);
        assert_eq!(presses(&b), 2, "press and release at the tap point");
    }

    #[test]
    fn drag_scrolls_instead_of_selecting() {
        let mut f = TouchScroll::default();
        let p = pos2(100.0, 100.0);
        frame(&mut f, vec![touch(1, TouchPhase::Start, p), press(p, true)], 0.0);
        let a = frame(&mut f, vec![touch(1, TouchPhase::Move, pos2(100.0, 130.0))], 0.05);
        assert_eq!(wheels(&a), vec![(Vec2::new(0.0, 30.0), TouchPhase::Start)]);
        let b = frame(&mut f, vec![touch(1, TouchPhase::Move, pos2(100.0, 150.0))], 0.1);
        assert_eq!(wheels(&b), vec![(Vec2::new(0.0, 20.0), TouchPhase::Move)]);
        let c = frame(&mut f, vec![touch(1, TouchPhase::End, pos2(100.0, 150.0)), press(pos2(100.0, 150.0), false)], 0.15);
        assert_eq!(wheels(&c), vec![(Vec2::ZERO, TouchPhase::End)]);
        assert_eq!(presses(&c), 0);
    }

    #[test]
    fn small_wobble_is_still_a_tap() {
        let mut f = TouchScroll::default();
        let p = pos2(100.0, 100.0);
        frame(&mut f, vec![touch(1, TouchPhase::Start, p)], 0.0);
        let a = frame(&mut f, vec![touch(1, TouchPhase::Move, pos2(103.0, 102.0))], 0.05);
        assert!(wheels(&a).is_empty());
    }

    #[test]
    fn long_press_selects_like_a_mouse() {
        let mut f = TouchScroll::default();
        let p = pos2(100.0, 100.0);
        frame(&mut f, vec![touch(1, TouchPhase::Start, p), press(p, true)], 0.0);
        let a = frame(&mut f, vec![], 0.6);
        assert_eq!(presses(&a), 1, "the held finger presses");
        let b = frame(&mut f, vec![touch(1, TouchPhase::Move, pos2(200.0, 100.0)), Event::PointerMoved(pos2(200.0, 100.0))], 0.7);
        assert!(wheels(&b).is_empty());
        assert!(b.iter().any(|e| matches!(e, Event::PointerMoved(_))));
        let c = frame(&mut f, vec![touch(1, TouchPhase::End, pos2(200.0, 100.0)), press(pos2(200.0, 100.0), false)], 0.8);
        assert_eq!(presses(&c), 1, "the release passes through");
    }

    #[test]
    fn edge_touch_passes_through_for_scroll_bars() {
        let mut f = TouchScroll::default();
        let p = pos2(990.0, 300.0);
        let a = frame(&mut f, vec![touch(1, TouchPhase::Start, p), press(p, true)], 0.0);
        assert_eq!(presses(&a), 1);
        let b = frame(&mut f, vec![touch(1, TouchPhase::Move, pos2(990.0, 400.0))], 0.1);
        assert!(wheels(&b).is_empty());
    }

    #[test]
    fn second_finger_is_ignored() {
        let mut f = TouchScroll::default();
        frame(&mut f, vec![touch(1, TouchPhase::Start, pos2(100.0, 100.0))], 0.0);
        let a = frame(&mut f, vec![touch(2, TouchPhase::Start, pos2(300.0, 100.0)), touch(2, TouchPhase::Move, pos2(300.0, 200.0))], 0.1);
        assert!(wheels(&a).is_empty());
    }
}
