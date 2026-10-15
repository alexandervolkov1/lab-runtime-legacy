//! Optional bounded renderer diagnostic. This is not a freshness, process-exit
//! or GPU deletion receipt. The existing Diagnostic envelope/credits are used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RendererProgress {
    pub(crate) event_loop: u64,
    pub(crate) render_owner: u64,
    pub(crate) submitted: u64,
    pub(crate) completed: u64,
    pub(crate) own_objects: u64,
    pub(crate) released_generations: u64,
    pub(crate) pending: bool,
}
impl RendererProgress {
    pub(crate) fn encode(self) -> [u8; 56] {
        let mut bytes = [0; 56];
        bytes[..4].copy_from_slice(b"LGR1");
        bytes[4] = u8::from(self.pending);
        for (chunk, value) in bytes[8..].chunks_exact_mut(8).zip([
            self.event_loop,
            self.render_owner,
            self.submitted,
            self.completed,
            self.own_objects,
            self.released_generations,
        ]) {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }
    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 56 || &bytes[..4] != b"LGR1" || bytes[4] > 1 || bytes[5..8] != [0; 3] {
            return None;
        }
        let value = |index: usize| {
            u64::from_le_bytes(bytes[8 + index * 8..16 + index * 8].try_into().unwrap())
        };
        let progress = Self {
            event_loop: value(0),
            render_owner: value(1),
            submitted: value(2),
            completed: value(3),
            own_objects: value(4),
            released_generations: value(5),
            pending: bytes[4] != 0,
        };
        (progress.completed <= progress.submitted).then_some(progress)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_is_bounded_and_never_implies_completion_from_submission() {
        let progress = RendererProgress {
            event_loop: 99,
            render_owner: 4,
            submitted: 3,
            completed: 2,
            pending: true,
            ..Default::default()
        };
        assert_eq!(RendererProgress::decode(&progress.encode()), Some(progress));
        let mut bytes = progress.encode();
        bytes[5] = 1;
        assert_eq!(RendererProgress::decode(&bytes), None);
        assert_eq!(
            RendererProgress::decode(
                &RendererProgress {
                    completed: 4,
                    ..progress
                }
                .encode()
            ),
            None
        );
        assert_eq!(
            RendererProgress::decode(b"unrelated legacy diagnostic"),
            None
        );
    }
}
