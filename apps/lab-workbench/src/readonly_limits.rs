//! Shared numeric admission limits. Reexports preserve all parent API values.
pub(crate) const JSON_BODY_BYTES: usize = 2_097_152;
pub(crate) const FRAME_BYTES: usize = 2_097_153;
pub(crate) const JSON_DEPTH: usize = 16;
pub(crate) const JSON_VALUES: usize = 16_384;
pub(crate) const JSON_STRING_BYTES: usize = 512;
pub(crate) const NAME_BYTES: usize = 64;
pub(crate) const PRESENTATION_FILE_BYTES: usize = 1024 * 1024;
