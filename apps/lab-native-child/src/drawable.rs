//! Independent native suspension causes. An event may clear only its own cause.

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Suspension {
    pub(super) occluded: bool,
    pub(super) minimized: bool,
    pub(super) unsupported_size: bool,
    pub(super) unsupported_scale: bool,
}

impl Suspension {
    pub(super) fn paused(self) -> bool {
        self.occluded || self.minimized || self.unsupported_size || self.unsupported_scale
    }

    pub(super) fn resize(&mut self, width: u32, height: u32) {
        self.unsupported_size = width != 1280 || height != 960;
    }

    pub(super) fn scale(&mut self, scale: f64) {
        self.unsupported_scale = scale != 1.;
    }
}

#[cfg(test)]
mod tests {
    use super::Suspension;

    #[test]
    fn unrelated_native_events_cannot_clear_a_suspension() {
        let mut state = Suspension::default();
        state.occluded = true;
        state.resize(1280, 960);
        state.scale(1.);
        assert!(state.paused());
        state.resize(0, 0);
        state.occluded = false;
        assert!(state.paused());
        state.scale(1.25);
        state.resize(1280, 960);
        assert!(state.paused());
        state.occluded = true;
        state.scale(1.);
        assert!(state.paused());
        state.occluded = false;
        assert!(!state.paused());
        state.minimized = true;
        state.resize(1280, 960);
        state.scale(1.);
        assert!(state.paused());
    }

    #[test]
    fn unsupported_and_nonfinite_scales_are_closed() {
        for scale in [0., -1., 1.25, 2., f64::NAN, f64::INFINITY] {
            let mut state = Suspension::default();
            state.scale(scale);
            assert!(state.paused());
        }
    }
}
