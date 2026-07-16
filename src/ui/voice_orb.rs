//! Audio-reactive orb for the voice HUD.
//!
//! Draws a warm amber disc with drifting translucent fog blobs whose motion
//! and brightness follow the live microphone level, so the orb visibly
//! "cooks" while speech is heard and settles to a faint haze in silence.

use std::cell::RefCell;
use std::f64::consts::TAU;
use std::rc::Rc;

use gtk::cairo;
use gtk::glib;
use gtk::prelude::*;

const ORB_SIZE: i32 = 54;
const FOG_BLOB_COUNT: usize = 5;
/// Seconds to rise toward a louder level (fast attack keeps speech snappy).
const ATTACK_SECONDS: f32 = 0.06;
/// Seconds to fall toward a quieter level (slow release keeps motion organic).
const RELEASE_SECONDS: f32 = 0.35;

/// Latest mic loudness supplier polled once per animation frame.
pub type LevelSource = Rc<dyn Fn() -> f32>;

struct OrbState {
    /// Raw target level in `0.0..=1.0` (pushed or polled).
    target_level: f32,
    /// Attack/release smoothed level actually rendered.
    smoothed_level: f32,
    /// Whether capture is active; inactive orbs settle to the idle haze.
    active: bool,
    /// Accumulated swirl phase; advances faster while speech is heard.
    swirl: f64,
    level_source: Option<LevelSource>,
    last_frame_time: Option<i64>,
    tick_id: Option<gtk::TickCallbackId>,
}

#[derive(Clone)]
pub struct VoiceOrb {
    area: gtk::DrawingArea,
    state: Rc<RefCell<OrbState>>,
}

impl VoiceOrb {
    pub fn new() -> Self {
        let area = gtk::DrawingArea::builder()
            .content_width(ORB_SIZE)
            .content_height(ORB_SIZE)
            .valign(gtk::Align::Center)
            .build();
        let state = Rc::new(RefCell::new(OrbState {
            target_level: 0.0,
            smoothed_level: 0.0,
            active: false,
            swirl: 0.0,
            level_source: None,
            last_frame_time: None,
            tick_id: None,
        }));

        let draw_state = state.clone();
        area.set_draw_func(move |_, context, width, height| {
            let state = draw_state.borrow();
            draw_orb(
                context,
                width,
                height,
                f64::from(state.smoothed_level),
                state.active,
                state.swirl,
            );
        });

        Self { area, state }
    }

    pub fn widget(&self) -> gtk::Widget {
        self.area.clone().upcast()
    }

    /// Install (or clear) the per-frame loudness supplier.
    pub fn set_level_source(&self, source: Option<LevelSource>) {
        let mut state = self.state.borrow_mut();
        state.level_source = source;
        if state.level_source.is_none() {
            state.target_level = 0.0;
        }
    }

    /// Marks capture active and keeps the animation running while revealed.
    pub fn set_active(&self, active: bool) {
        {
            let mut state = self.state.borrow_mut();
            state.active = active;
            if !active {
                state.target_level = 0.0;
            }
        }
        self.area.queue_draw();
    }

    /// Starts the per-frame animation; call when the HUD becomes visible.
    pub fn start_animation(&self) {
        let mut state = self.state.borrow_mut();
        if state.tick_id.is_some() {
            return;
        }
        state.last_frame_time = None;
        let tick_state = self.state.clone();
        state.tick_id = Some(self.area.add_tick_callback(move |area, clock| {
            let mut state = tick_state.borrow_mut();
            let frame_time = clock.frame_time();
            let dt = state
                .last_frame_time
                .map(|last| ((frame_time - last).max(0) as f32) / 1_000_000.0)
                .unwrap_or(0.0);
            state.last_frame_time = Some(frame_time);

            if let Some(source) = state.level_source.clone() {
                state.target_level = source().clamp(0.0, 1.0);
            }
            let target = if state.active {
                state.target_level
            } else {
                0.0
            };
            state.smoothed_level = smooth_level(state.smoothed_level, target, dt);
            state.swirl += f64::from(dt) * swirl_speed(state.smoothed_level);
            drop(state);
            area.queue_draw();
            glib::ControlFlow::Continue
        }));
    }

    /// Stops the animation and freezes on a calm frame; call when hidden.
    pub fn stop_animation(&self) {
        let mut state = self.state.borrow_mut();
        if let Some(id) = state.tick_id.take() {
            id.remove();
        }
        state.last_frame_time = None;
        state.smoothed_level = 0.0;
        drop(state);
        self.area.queue_draw();
    }
}

/// Frame-rate independent attack/release smoothing toward `target`.
fn smooth_level(current: f32, target: f32, dt: f32) -> f32 {
    let time_constant = if target > current {
        ATTACK_SECONDS
    } else {
        RELEASE_SECONDS
    };
    if time_constant <= 0.0 || dt <= 0.0 {
        return target.clamp(0.0, 1.0);
    }
    let alpha = 1.0 - (-dt / time_constant).exp();
    (current + (target - current) * alpha).clamp(0.0, 1.0)
}

/// Swirl phase speed in radians/second; idle drift stays barely perceptible.
fn swirl_speed(level: f32) -> f64 {
    0.35 + 3.2 * f64::from(level.clamp(0.0, 1.0))
}

/// Fog opacity for a given smoothed level; a faint haze survives silence so
/// the orb never looks dead while capture is active.
fn fog_alpha(level: f64, active: bool) -> f64 {
    let base = if active { 0.10 } else { 0.05 };
    (base + 0.45 * level.clamp(0.0, 1.0)).min(0.6)
}

fn draw_orb(
    context: &cairo::Context,
    width: i32,
    height: i32,
    level: f64,
    active: bool,
    swirl: f64,
) {
    let width = f64::from(width);
    let height = f64::from(height);
    let center_x = width / 2.0;
    let center_y = height / 2.0;
    let radius = (width.min(height) / 2.0) - 1.0;
    if radius <= 0.0 {
        return;
    }

    context.save().ok();
    context.arc(center_x, center_y, radius, 0.0, TAU);
    context.clip();

    // Dark backing keeps the translucent fog readable over any terminal.
    context.set_source_rgba(0.09, 0.06, 0.03, 0.92);
    context.paint().ok();

    // Warm amber body: cream core -> amber -> deep copper rim.
    let body = radial(center_x, center_y, radius, |gradient| {
        gradient.add_color_stop_rgba(0.0, 1.0, 0.878, 0.639, 0.85);
        gradient.add_color_stop_rgba(0.52, 0.941, 0.702, 0.294, 0.40);
        gradient.add_color_stop_rgba(1.0, 0.235, 0.125, 0.047, 0.70);
    });
    context.set_source(&body).ok();
    context.paint().ok();

    // Drifting fog blobs, brighter and faster with louder speech.
    let alpha = fog_alpha(level, active);
    for blob in 0..FOG_BLOB_COUNT {
        let phase = (blob as f64 / FOG_BLOB_COUNT as f64) * TAU;
        let angle = phase + swirl * (0.6 + 0.15 * blob as f64);
        let wobble = (swirl * 0.7 + phase * 3.0).sin();
        let distance = radius * (0.20 + 0.16 * wobble.abs() + 0.14 * level);
        let blob_x = center_x + angle.cos() * distance;
        let blob_y = center_y + angle.sin() * distance;
        let blob_radius = radius * (0.30 + 0.10 * (phase + swirl).cos().abs() + 0.22 * level);
        let blob_alpha = alpha * (0.6 + 0.4 * ((swirl * 1.3 + phase * 2.0).sin() * 0.5 + 0.5));
        let fog = radial(blob_x, blob_y, blob_radius, |gradient| {
            gradient.add_color_stop_rgba(0.0, 1.0, 0.925, 0.784, blob_alpha);
            gradient.add_color_stop_rgba(1.0, 1.0, 0.925, 0.784, 0.0);
        });
        context.set_source(&fog).ok();
        context.paint().ok();
    }

    // Soft top sheen for the glassy finish.
    let sheen = radial(
        center_x - radius * 0.35,
        center_y - radius * 0.45,
        radius * 0.9,
        |gradient| {
            gradient.add_color_stop_rgba(0.0, 1.0, 1.0, 0.97, 0.18);
            gradient.add_color_stop_rgba(1.0, 1.0, 1.0, 0.97, 0.0);
        },
    );
    context.set_source(&sheen).ok();
    context.paint().ok();

    context.restore().ok();
}

fn radial(
    center_x: f64,
    center_y: f64,
    radius: f64,
    add_stops: impl Fn(&cairo::RadialGradient),
) -> cairo::RadialGradient {
    let gradient = cairo::RadialGradient::new(center_x, center_y, 0.0, center_x, center_y, radius);
    add_stops(&gradient);
    gradient
}

#[cfg(test)]
mod tests {
    use super::{fog_alpha, smooth_level, swirl_speed};

    #[test]
    fn smoothing_attacks_faster_than_it_releases() {
        let rising = smooth_level(0.0, 1.0, 0.05);
        let falling = 1.0 - smooth_level(1.0, 0.0, 0.05);
        assert!(rising > falling, "rising {rising} vs falling {falling}");
        assert!(rising > 0.5, "attack should be mostly complete in 50ms");
        assert!(falling < 0.2, "release should still be mostly full in 50ms");
    }

    #[test]
    fn smoothing_stays_clamped_and_converges() {
        let mut level = 0.0;
        for _ in 0..200 {
            level = smooth_level(level, 1.0, 0.016);
        }
        assert!(level > 0.99 && level <= 1.0);
        for _ in 0..200 {
            level = smooth_level(level, 0.0, 0.016);
        }
        assert!((0.0..0.01).contains(&level));
        assert_eq!(smooth_level(0.3, 2.0, 0.0), 1.0);
    }

    #[test]
    fn fog_and_swirl_scale_with_level() {
        assert!(fog_alpha(0.0, true) > fog_alpha(0.0, false));
        assert!(fog_alpha(1.0, true) > fog_alpha(0.2, true));
        assert!(fog_alpha(1.0, true) <= 0.6);
        assert!(swirl_speed(1.0) > swirl_speed(0.0));
    }
}
