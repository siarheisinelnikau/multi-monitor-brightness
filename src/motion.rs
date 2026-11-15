//! Finite cosmetic interpolation. No timers, workers, or brightness commands live here.
#[derive(Clone, Copy, Debug)]
struct Tween {
    from: f32,
    target: f32,
    started: f64,
    sampled: f64,
}

impl Tween {
    fn new(value: f32, now: f64) -> Self {
        Self {
            from: value,
            target: value,
            started: now,
            sampled: now,
        }
    }

    fn at(&self, now: f64, duration: f64) -> f32 {
        let t = ((now - self.started) / duration).clamp(0.0, 1.0) as f32;
        let eased = 1.0 - (1.0 - t).powi(3);
        self.from + (self.target - self.from) * eased
    }

    fn sample(&mut self, target: f32, now: f64, immediate: bool) -> (f32, bool) {
        const DURATION: f64 = 0.16;
        // A hidden/clipped control resumes at its current value; it never replays old motion.
        if immediate || now - self.sampled > 0.4 || now < self.sampled {
            *self = Self::new(target, now);
        } else if self.target != target {
            self.from = self.at(now, DURATION);
            self.target = target;
            self.started = now;
        }
        self.sampled = now;
        let active = self.from != self.target && now < self.started + DURATION;
        if active {
            (self.at(now, DURATION), true)
        } else {
            self.from = self.target;
            (self.target, false)
        }
    }
}

pub fn value(ui: &egui::Ui, id: egui::Id, target: f32, immediate: bool) -> f32 {
    let now = ui.input(|i| i.time);
    let id = id.with(("finite-motion", ui.ctx().viewport_id()));
    let (value, active) = ui.ctx().data_mut(|data| {
        let state = data.get_temp_mut_or_insert_with(id, || Tween::new(target, now));
        state.sample(target, now, immediate)
    });
    if active {
        ui.ctx().request_repaint();
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_converges_exactly_and_stops_requesting_frames() {
        let mut tween = Tween::new(20.0, 0.0);
        assert!(tween.sample(80.0, 0.01, false).1);
        let (middle, active) = tween.sample(80.0, 0.08, false);
        assert!(middle > 20.0 && middle < 80.0 && active);
        assert_eq!(tween.sample(80.0, 0.18, false), (80.0, false));
        for n in 2..100 {
            assert_eq!(tween.sample(80.0, n as f64, false), (80.0, false));
        }
    }

    #[test]
    fn direct_manipulation_reduced_motion_and_hidden_gaps_snap() {
        let mut tween = Tween::new(20.0, 0.0);
        assert_eq!(tween.sample(80.0, 0.01, true), (80.0, false));
        assert!(tween.sample(30.0, 0.02, false).1);
        assert_eq!(tween.sample(50.0, 5.0, false), (50.0, false));
        assert_eq!(tween.sample(60.0, 5.01, true), (60.0, false));
    }

    #[test]
    fn retargeting_preserves_visual_continuity_and_still_settles() {
        let mut tween = Tween::new(0.0, 0.0);
        tween.sample(100.0, 0.01, false);
        let before = tween.sample(100.0, 0.06, false).0;
        let after = tween.sample(25.0, 0.06, false).0;
        assert!((before - after).abs() < f32::EPSILON);
        assert_eq!(tween.sample(25.0, 0.23, false), (25.0, false));
    }
}
