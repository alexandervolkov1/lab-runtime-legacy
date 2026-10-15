//! Parent offline conversion/export extend the shared pure admission surface.
include!("consumer.rs");

mod conversion;
mod export;
mod source_v1;
pub(crate) use conversion::{ConversionError, ValidatedV1Document, convert_v1};
pub(crate) use export::{ExportError, export_v1};

#[cfg(test)]
pub(crate) mod admission_fixtures;
#[cfg(test)]
mod admission_tests;
#[cfg(test)]
mod canonical_tests;
#[cfg(test)]
mod conversion_fixtures;
#[cfg(test)]
mod conversion_model_tests;
#[cfg(test)]
mod conversion_tests;
#[cfg(test)]
mod export_fixtures;
#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod structure_tests;
#[cfg(test)]
mod tests;
