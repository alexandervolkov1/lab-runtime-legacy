//! One original Job child; no restart, fallback, Runtime or IPC worker.
use crate::{Result, write};
use lab_windows_job::{Job, Limits};
use serde_json::json;
use std::{
    path::Path,
    time::{Duration, Instant},
};

// Selected before EFRAME-1 trials. Experimental OS commit ceiling, not a
// production policy, Rust budget, VRAM budget, or inherited Glow/WGPU cap.
const CAP: usize = 1_073_741_824;

pub(super) fn run(renderer: &str, dir: &Path) -> Result<()> {
    if !matches!(renderer, "wgpu" | "glow") || !dir.is_absolute() || dir.exists() {
        return Err("explicit wgpu|glow and new absolute receipt directory required".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut job = Job::new(Limits {
        process_commit: CAP,
        job_commit: CAP,
    })
    .map_err(|e| format!("{e:?}"))?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let launch = job.prepare(
        &exe,
        &[
            "--renderer",
            renderer,
            "--receipts",
            dir.to_str().ok_or("UTF-8 path")?,
        ],
    );
    if let Err(error) = launch {
        write(
            &dir.join("launch-error.json"),
            &json!({"error":format!("{error:?}")}),
        );
        if job.objects().processes != 0 {
            job.rollback(91).map_err(|e| format!("{e:?}"))?;
        }
        return Err(format!("{error:?}"));
    }
    job.verify_membership().map_err(|e| format!("{e:?}"))?;
    job.verify_limits().map_err(|e| format!("{e:?}"))?;
    write(
        &dir.join("admission.json"),
        &json!({"pid":job.pid(),"renderer":renderer,
        "process_cap":CAP,"job_cap":CAP,"suspended_membership_and_limits_verified":true}),
    );
    job.resume().map_err(|e| format!("{e:?}"))?;
    let start = Instant::now();
    let mut contained = false;
    let exit = loop {
        if let Some(code) = job.poll_exit().map_err(|e| format!("{e:?}"))? {
            break code;
        }
        if !contained && (start.elapsed() > Duration::from_secs(90) || dir.join("contain").exists())
        {
            job.terminate(92).map_err(|e| format!("{e:?}"))?;
            contained = true;
        }
        if start.elapsed() > Duration::from_secs(95) {
            return Err("original process exit unconfirmed; no replacement permitted".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let peak = job.verify_limits().map_err(|e| format!("{e:?}"))?;
    job.retire().map_err(|e| format!("{e:?}"))?;
    let handles = job.objects();
    let retired = handles == lab_windows_job::Objects::default();
    let normal =
        exit == 0 && dir.join("returned.json").exists() && dir.join("on-exit.json").exists();
    let expected_containment = contained && exit == 92 && dir.join("contain").exists();
    let expected_crash = !contained && exit != 0 && dir.join("crash-entered.json").exists();
    let passed = retired
        && (normal || expected_containment || expected_crash)
        && peak.peak_process_commit <= CAP
        && peak.peak_job_commit <= CAP;
    write(
        &dir.join("process.json"),
        &json!({"renderer":renderer,"exit":exit,"contained":contained,
        "normal":normal,"passed":passed,"peak_process_commit":peak.peak_process_commit,
        "peak_job_commit":peak.peak_job_commit,"cap":CAP,"original_exit_confirmed":true,
        "original_handles_retired":retired,"remaining_owned_job_process_thread_handles":[handles.jobs,handles.processes,handles.threads],
        "ipc_obligations":"none; test receipt files only","native_gpu_cleanup_claimed":false}),
    );
    if passed {
        Ok(())
    } else {
        Err("unexpected child outcome; retain all receipts".into())
    }
}
