//! Qualification-only injection into the original audited Device seams.
//! These are artificial faults, not observations of actual driver/device loss.
use crate::{
    device::Device,
    readonly_v2::{
        completion::{Completion, WaitFailure},
        deletion::{DeleteDisposition, ObjectKind},
    },
};

pub(super) const MODES: &[&str] = &[
    "gpu-timeout-proof",
    "gpu-wait-failed",
    "gpu-device-loss",
    "gpu-native-teardown",
    "gpu-original-error",
    "gpu-texture-before",
    "gpu-texture-after",
    "gpu-buffer-before",
    "gpu-buffer-after",
    "gpu-shader-before",
    "gpu-shader-after",
    "gpu-program-before",
    "gpu-program-after",
    "gpu-vao-before",
    "gpu-vao-after",
    "gpu-fence-before",
    "gpu-fence-after",
];

pub(super) fn arm(device: &mut Device, mode: &str) {
    match mode {
        "gpu-timeout-proof" => device.wait_fault = Some(Completion::Timeout),
        "gpu-wait-failed" => device.wait_fault = Some(Completion::Failed(WaitFailure::WaitError)),
        "gpu-device-loss" => device.wait_fault = Some(Completion::DeviceLost),
        "gpu-native-teardown" => device.teardown_fault = true,
        "gpu-original-error" => {
            device.delete_fault = Some((ObjectKind::Shader, DeleteDisposition::Ambiguous, 1))
        }
        _ => {
            let kind = if mode.starts_with("gpu-texture-") {
                ObjectKind::Texture
            } else if mode.starts_with("gpu-buffer-") {
                ObjectKind::Buffer
            } else if mode.starts_with("gpu-shader-") {
                ObjectKind::Shader
            } else if mode.starts_with("gpu-program-") {
                ObjectKind::Program
            } else if mode.starts_with("gpu-vao-") {
                ObjectKind::VertexArray
            } else if mode.starts_with("gpu-fence-") {
                ObjectKind::Fence
            } else {
                return;
            };
            device.delete_fault = Some((
                kind,
                if mode.ends_with("-before") {
                    DeleteDisposition::NotStarted
                } else {
                    DeleteDisposition::Ambiguous
                },
                1,
            ));
        }
    }
}
