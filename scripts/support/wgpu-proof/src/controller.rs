//! One suspended Job child at a time, with original-handle retirement.
use crate::{Result, write};
use lab_windows_job::{Job, Limits};
use serde_json::json;
use std::{
    path::Path,
    time::{Duration, Instant},
};

const CAP: usize = 805_306_368;
const LOW_CAP: usize = 16_777_216;

pub(super) fn run(dir: &Path) -> Result<()> {
    if !dir.is_absolute() || dir.exists() {
        return Err("Use a new absolute evidence directory".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut records = Vec::new();
    for case in [
        "frames",
        "glow",
        "no-adapter",
        "unsupported-format",
        "write-drain",
        "validation",
        "partial-error",
        "map-error",
        "device-destroy",
        "delayed",
        "timeout",
        "low-cap",
    ] {
        let output = dir.join(case);
        std::fs::create_dir(&output).map_err(|e| e.to_string())?;
        let cap = if case == "low-cap" { LOW_CAP } else { CAP };
        let mut job = Job::new(Limits {
            process_commit: cap,
            job_commit: cap,
        })
        .map_err(|e| format!("{e:?}"))?;
        let start = Instant::now();
        let launch = job.prepare(
            &exe,
            &["--child", case, output.to_str().ok_or("UTF-8 path")?],
        );
        let mut launch_error = launch.err().map(|e| format!("{e:?}"));
        let mut admitted = false;
        let mut contained = false;
        if launch_error.is_none() {
            job.verify_membership().map_err(|e| format!("{e:?}"))?;
            job.verify_limits().map_err(|e| format!("{e:?}"))?;
            admitted = true;
            write(
                &output.join("admission.json"),
                &json!({"pid":job.pid(), "suspended_membership_verified":true, "process_cap":cap, "job_cap":cap}),
            );
            if let Err(error) = job.resume() {
                launch_error = Some(format!("{error:?}"));
                job.rollback(91).map_err(|e| format!("{e:?}"))?;
            }
        } else if job.objects().processes != 0 {
            job.rollback(91).map_err(|e| format!("{e:?}"))?;
        }
        let exit = loop {
            if job.objects().processes == 0 {
                break None;
            }
            if let Some(code) = job.poll_exit().map_err(|e| format!("{e:?}"))? {
                break Some(code);
            }
            if start.elapsed() > Duration::from_secs(30) && !contained {
                job.terminate(92).map_err(|e| format!("{e:?}"))?;
                contained = true;
            }
            if start.elapsed() > Duration::from_secs(35) {
                return Err(format!("{case}: original exit unconfirmed; no next child"));
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let peak = job.verify_limits().map_err(|e| format!("{e:?}"))?;
        job.retire().map_err(|e| format!("{e:?}"))?;
        let objects = job.objects();
        if objects.jobs != 0 || objects.processes != 0 || objects.threads != 0 {
            return Err("original retirement failed".into());
        }
        let success = output.join("result.json").exists() && exit == Some(0);
        let expected = match case {
            "timeout" => contained && exit == Some(92) && output.join("pending.json").exists(),
            "low-cap" => !success && (launch_error.is_some() || exit.is_some_and(|code| code != 0)),
            _ => success,
        };
        let record = json!({"case":case,"process_cap":cap,"job_cap":cap,"admitted":admitted,"launch_error":launch_error,"exit":exit,"contained":contained,"elapsed_ms":start.elapsed().as_millis(),"peak_process_commit":peak.peak_process_commit,"peak_job_commit":peak.peak_job_commit,"original_process_retired":true,"ipc_obligations":"none: bounded receipt files only","expected_outcome":expected});
        write(&output.join("process.json"), &record);
        println!("{record}");
        records.push(record);
        if !expected {
            write(&dir.join("controller.json"), &json!(records));
            return Err(format!("unexpected {case} outcome"));
        }
    }
    write(&dir.join("controller.json"), &json!(records));
    Ok(())
}
