//! Capability admission for the existing fixed-size desktop GL renderer.
//! Diagnostic strings never participate in admission. This module performs no GL
//! calls and grants no GPU ownership, allocation success or completion evidence.

use serde::Serialize;
use std::fmt;

/// Entry points used by Device and by the pinned Glow context loader.
pub const REQUIRED_SYMBOLS: &[&str] = &[
    "glGetString",
    "glGetStringi",
    "glGetIntegerv",
    "glGetError",
    "glCreateShader",
    "glShaderSource",
    "glCompileShader",
    "glGetShaderiv",
    "glGetShaderInfoLog",
    "glDeleteShader",
    "glIsShader",
    "glCreateProgram",
    "glAttachShader",
    "glLinkProgram",
    "glGetProgramiv",
    "glGetProgramInfoLog",
    "glDeleteProgram",
    "glIsProgram",
    "glGenVertexArrays",
    "glBindVertexArray",
    "glDeleteVertexArrays",
    "glIsVertexArray",
    "glGenBuffers",
    "glBindBuffer",
    "glBufferData",
    "glBufferSubData",
    "glGetBufferParameteriv",
    "glDeleteBuffers",
    "glIsBuffer",
    "glEnableVertexAttribArray",
    "glVertexAttribPointer",
    "glGenTextures",
    "glBindTexture",
    "glTexParameteri",
    "glTexImage2D",
    "glTexSubImage2D",
    "glGetTexLevelParameteriv",
    "glDeleteTextures",
    "glIsTexture",
    "glPixelStorei",
    "glActiveTexture",
    "glBindFramebuffer",
    "glCheckFramebufferStatus",
    "glGetFramebufferAttachmentParameteriv",
    "glViewport",
    "glScissor",
    "glEnable",
    "glDisable",
    "glColorMask",
    "glClearColor",
    "glClear",
    "glBlendEquation",
    "glBlendFunc",
    "glUseProgram",
    "glGetUniformLocation",
    "glUniform1i",
    "glDrawElements",
    "glFenceSync",
    "glIsSync",
    "glClientWaitSync",
    "glDeleteSync",
    "glFlush",
    "glReadBuffer",
    "glReadPixels",
];

/// A supported API can still fail later allocation, compilation or execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum FailureKind {
    /// A required API or numerical capability is unavailable.
    UnsupportedCapability,
    /// Initialization failed without proving an absent graphics capability.
    InitializationFailure,
}

/// Names are deliberately absent from the numerical admission input.
#[derive(Clone, Debug, Serialize)]
pub struct Capabilities {
    /// Desktop version parsed by the pinned Glow loader.
    pub version: [u32; 2],
    /// OpenGL ES is outside this desktop renderer contract.
    pub embedded: bool,
    /// Actual GL_CONTEXT_PROFILE_MASK, including the core bit (0x1).
    pub profile_mask: i32,
    /// Actual GL_MAX_TEXTURE_SIZE; the existing atlas requires at least 2048.
    pub max_texture_size: i32,
    /// Actual GL_MAX_VIEWPORT_DIMS.
    pub max_viewport: [i32; 2],
    /// Actual default back-left color attachment channel sizes.
    pub color_bits: [i32; 4],
    /// The renderer reads BACK and presents the same double-buffered surface.
    pub double_buffered: bool,
    /// GL_FRAMEBUFFER_COMPLETE for framebuffer zero.
    pub default_framebuffer_complete: bool,
}

/// Capability failures have one stable diagnostic category.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsupportedCapability(pub &'static str);
impl fmt::Display for UnsupportedCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "UnsupportedCapability: {}", self.0)
    }
}

impl Capabilities {
    /// Checks only API/numerical requirements; no hardware identity is accepted.
    pub fn admit(&self) -> Result<(), UnsupportedCapability> {
        let requirement = if self.embedded || self.version < [3, 3] {
            Some("desktop OpenGL 3.3 or newer")
        } else if self.profile_mask & 1 == 0 {
            Some("OpenGL core profile")
        } else if self.max_texture_size < 2048 {
            Some("GL_MAX_TEXTURE_SIZE >= 2048")
        } else if self.max_viewport[0] < 1280 || self.max_viewport[1] < 960 {
            Some("GL_MAX_VIEWPORT_DIMS >= 1280x960")
        } else if self.color_bits.iter().any(|bits| *bits < 8) {
            Some("default framebuffer RGBA channels >= 8 bits")
        } else if !self.double_buffered {
            Some("double-buffered default framebuffer")
        } else if !self.default_framebuffer_complete {
            Some("complete default framebuffer")
        } else {
            None
        };
        requirement.map_or(Ok(()), |reason| Err(UnsupportedCapability(reason)))
    }
}

/// Reject a missing entry point before Glow can call an unloaded function.
pub fn require_symbols(
    mut available: impl FnMut(&str) -> bool,
) -> Result<(), UnsupportedCapability> {
    for symbol in REQUIRED_SYMBOLS {
        if !available(symbol) {
            return Err(UnsupportedCapability(symbol));
        }
    }
    Ok(())
}

/// Retained graphics identity is diagnostic only, with bounded string lengths.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Diagnostics {
    /// Actual glutin platform API (WGL or EGL in the current Windows harness).
    pub platform_api: &'static str,
    /// Actual GL_VENDOR.
    pub vendor: String,
    /// Actual GL_RENDERER; installed-adapter inventory is not a substitute.
    pub renderer: String,
    /// Actual GL_VERSION.
    pub version: String,
    /// Actual GL_SHADING_LANGUAGE_VERSION.
    pub shading_language_version: String,
    /// glutin's selected framebuffer configuration, bounded for retention.
    pub framebuffer_config: String,
    /// Actual context flags, after the desktop version check.
    pub context_flags: i32,
}

/// One initialization record, also emitted on failure before any own GPU objects.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// Last attempted stage; errors are retained at the originating stage.
    pub stage: &'static str,
    /// Diagnostic identity is kept separate from the admission input.
    pub diagnostics: Diagnostics,
    /// Queried capabilities, absent when context/loader initialization failed.
    pub capabilities: Option<Capabilities>,
    /// Current PORT-1 fixed physical framebuffer bound.
    pub drawable: [usize; 2],
    /// Actual native scale; PORT-2 has not expanded the scale-one contract.
    pub scale: Option<f64>,
    /// Number of required entry points checked on this context.
    pub required_entry_points: usize,
    /// True only after all initialization and profile checks passed.
    pub admitted: bool,
    /// Structured rejection class, independent of hardware names.
    pub error_kind: Option<FailureKind>,
    /// Original initialization diagnostic, not a GPU cleanup receipt.
    pub error: Option<String>,
}
impl Default for Report {
    fn default() -> Self {
        Self {
            stage: "window_display",
            diagnostics: Diagnostics::default(),
            capabilities: None,
            drawable: [1280, 960],
            scale: None,
            required_entry_points: 0,
            admitted: false,
            error_kind: None,
            error: None,
        }
    }
}

/// Original structured startup failure; no created GL payload is hidden in it.
#[derive(Debug)]
pub struct InitializationError {
    /// Original initialization record, including any available GL identity.
    pub report: Box<Report>,
}
impl fmt::Display for InitializationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:?} at {}: {}",
            self.report
                .error_kind
                .unwrap_or(FailureKind::InitializationFailure),
            self.report.stage,
            self.report
                .error
                .as_deref()
                .unwrap_or("unknown initialization failure")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn supported() -> Capabilities {
        Capabilities {
            version: [3, 3],
            embedded: false,
            profile_mask: 1,
            max_texture_size: 2048,
            max_viewport: [1280, 960],
            color_bits: [8; 4],
            double_buffered: true,
            default_framebuffer_complete: true,
        }
    }
    #[test]
    fn exact_minima_and_newer_desktop_core_are_admitted() {
        supported().admit().unwrap();
        let mut newer = supported();
        newer.version = [4, 6];
        newer.admit().unwrap();
    }
    #[test]
    fn missing_api_profile_and_numerical_capabilities_are_typed_rejections() {
        let candidates: Vec<Capabilities> = vec![
            Capabilities {
                version: [3, 2],
                ..supported()
            },
            Capabilities {
                embedded: true,
                ..supported()
            },
            Capabilities {
                profile_mask: 2,
                ..supported()
            },
            Capabilities {
                max_texture_size: 2047,
                ..supported()
            },
            Capabilities {
                max_viewport: [1279, 960],
                ..supported()
            },
            Capabilities {
                max_viewport: [1280, 959],
                ..supported()
            },
            Capabilities {
                color_bits: [8, 8, 8, 0],
                ..supported()
            },
            Capabilities {
                double_buffered: false,
                ..supported()
            },
            Capabilities {
                default_framebuffer_complete: false,
                ..supported()
            },
        ];
        for candidate in candidates {
            let error = candidate.admit().unwrap_err();
            assert!(error.to_string().starts_with("UnsupportedCapability:"));
        }
    }
    #[test]
    fn every_required_symbol_is_checked_before_loader_use() {
        require_symbols(|_| true).unwrap();
        for missing in REQUIRED_SYMBOLS {
            assert_eq!(
                require_symbols(|name| name != *missing),
                Err(UnsupportedCapability(missing))
            );
        }
    }
    #[test]
    fn diagnostic_identity_never_affects_admission() {
        for renderer in ["AMD new driver", "NVIDIA", "Mesa", "unknown vendor"] {
            let report = Report {
                diagnostics: Diagnostics {
                    renderer: renderer.into(),
                    ..Default::default()
                },
                capabilities: Some(supported()),
                ..Default::default()
            };
            report.capabilities.as_ref().unwrap().admit().unwrap();
        }
    }
    #[test]
    fn startup_errors_preserve_category_stage_and_available_identity() {
        for kind in [
            FailureKind::UnsupportedCapability,
            FailureKind::InitializationFailure,
        ] {
            let error = InitializationError {
                report: Box::new(Report {
                    stage: "entry_points",
                    error_kind: Some(kind),
                    error: Some("original reason".into()),
                    diagnostics: Diagnostics {
                        renderer: "actual context identity".into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            };
            let encoded = serde_json::to_value(&error.report).unwrap();
            assert_eq!(encoded["stage"], "entry_points");
            assert_eq!(encoded["admitted"], false);
            assert_eq!(
                encoded["diagnostics"]["renderer"],
                "actual context identity"
            );
            assert_eq!(encoded["error_kind"], format!("{kind:?}"));
            assert!(error.to_string().contains("original reason"));
        }
    }
}
