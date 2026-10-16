//! Finite, CPU-only MP-1 experiments through the production Job launch path.
#![forbid(unsafe_code)]
#![cfg_attr(feature = "windowed", windows_subsystem = "windows")]

use lab_windows_job::{
    Job, Limits,
    accounting::{self, ProcessMemory, Reservation, Snapshot},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

const MIB: usize = 1024 * 1024;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn memory(value: std::result::Result<ProcessMemory, lab_windows_job::Error>) -> Value {
    match value {
        Ok(p) => json!({"cb":p.cb,"fault_count":p.faults,"peak_working_set":p.peak_working_set,
            "working_set":p.working_set,"peak_paged_pool":p.peak_paged_pool,"paged_pool":p.paged_pool,
            "peak_nonpaged_pool":p.peak_nonpaged_pool,"nonpaged_pool":p.nonpaged_pool,
            "pagefile_usage":p.pagefile,"peak_pagefile_usage":p.peak_pagefile,"private_usage":p.private}),
        Err(e) => json!({"error":e.operation,"code":e.code}),
    }
}
fn snapshot(s: Snapshot) -> Value {
    let members: Vec<Value> = s
        .members
        .into_iter()
        .map(|m| match m.reading {
            Ok((image, p)) => json!({"pid":m.pid,"image":image,"memory":memory(Ok(p))}),
            Err(e) => json!({"pid":m.pid,"error":e.operation,"code":e.code}),
        })
        .collect();
    json!({"extended_bytes":s.extended_bytes,"accounting_bytes":s.accounting_bytes,"pid_bytes":s.pid_bytes,
        "active_process_limit":s.active_limit,"flags":s.flags,"process_limit":s.process_limit,
        "job_limit":s.job_limit,"peak_process":s.peak_process,"peak_job":s.peak_job,
        "total_processes":s.total,"active_processes":s.active,"limit_terminated_processes":s.terminated,
        "pids":s.pids,"members":members,"original_process_member":s.member,"K32GetProcessMemoryInfo":memory(s.process)})
}
fn kernel<T>(value: std::result::Result<T, lab_windows_job::Error>) -> Result<T> {
    value.map_err(|e| format!("{}: {}", e.operation, e.code).into())
}
fn write(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
fn phase(
    dir: &Path,
    sequence: &mut u32,
    name: &str,
    detail: Value,
    started: Instant,
) -> Result<()> {
    let value = json!({"sequence":*sequence,"phase":name,"detail":detail,
        "child_elapsed_us":started.elapsed().as_micros(),
        "self_K32GetProcessMemoryInfo":memory(accounting::current_process_memory())});
    let ready = dir.join(format!("{sequence:03}.json"));
    let temporary = dir.join("phase.tmp");
    write(&temporary, &value)?;
    fs::rename(temporary, ready)?;
    let ack = dir.join(format!("{sequence:03}.ack"));
    while !ack.exists() {
        if started.elapsed() > Duration::from_secs(20) {
            return Err("child watchdog".into());
        }
        thread::sleep(Duration::from_millis(2));
    }
    *sequence += 1;
    Ok(())
}
fn child(dir: &Path, step: usize) -> Result<()> {
    if ![MIB, 4 * MIB, 16 * MIB].contains(&step) {
        return Err("step bound".into());
    }
    let start = Instant::now();
    let mut sequence = 0;
    phase(dir, &mut sequence, "initialized", json!({}), start)?;
    let mut allocation = kernel(Reservation::reserve(128 * MIB))?;
    phase(
        dir,
        &mut sequence,
        "reserved",
        json!({"reserved":128*MIB,"committed":0,"next_region":kernel(allocation.next_region())?}),
        start,
    )?;
    let mut committed = 0;
    let mut denied = false;
    for _ in 0..64 {
        match allocation.commit(step) {
            Ok(()) => {
                committed += step;
                phase(
                    dir,
                    &mut sequence,
                    "committed_untouched",
                    json!({"committed":committed,"request":step}),
                    start,
                )?;
                allocation.touch();
                phase(
                    dir,
                    &mut sequence,
                    "touched",
                    json!({"committed":committed}),
                    start,
                )?;
            }
            Err(e) => {
                phase(
                    dir,
                    &mut sequence,
                    "allocation_denied",
                    json!({"committed":committed,"request":step,"operation":e.operation,"error":e.code,"next_region":kernel(allocation.next_region())?}),
                    start,
                )?;
                denied = true;
                break;
            }
        }
        if committed >= 96 * MIB {
            break;
        }
    }
    kernel(allocation.decommit())?;
    phase(
        dir,
        &mut sequence,
        "decommitted",
        json!({"reserved":128*MIB,"committed":0,"next_region":kernel(allocation.next_region())?}),
        start,
    )?;
    kernel(allocation.release())?;
    phase(dir, &mut sequence, "released", json!({}), start)?;
    if !denied {
        return Err("bounded test did not reach denial".into());
    }
    Ok(())
}
fn run_case(
    exe: &Path,
    dir: &Path,
    process: usize,
    aggregate: usize,
    step: usize,
    graphical: bool,
) -> Result<Value> {
    if !exe.is_absolute() || !dir.is_absolute() {
        return Err("absolute paths required".into());
    }
    fs::create_dir(dir)?;
    let mut job = kernel(Job::new(Limits {
        process_commit: process,
        job_commit: aggregate,
    }))?;
    let mut port = kernel(job.accounting_notifications())?;
    let output = dir.to_str().ok_or("UTF8 output")?;
    let step_text = step.to_string();
    let args = if graphical {
        vec!["--child", "low-cap", output]
    } else {
        vec!["--child", output, &step_text]
    };
    kernel(job.prepare(exe, &args))?;
    let mut samples = Vec::new();
    let start = Instant::now();
    samples.push(
        json!({"phase":"suspended_before_ResumeThread","elapsed_us":start.elapsed().as_micros(),
        "snapshot":snapshot(kernel(job.accounting_snapshot())?),"messages":kernel(port.drain())?}),
    );
    kernel(job.resume())?;
    let mut next = 0;
    let mut last_graph = Instant::now();
    let mut terminated = false;
    let exit = loop {
        if let Some(exit) = kernel(job.poll_exit())? {
            break exit;
        }
        if graphical {
            if last_graph.elapsed() >= Duration::from_millis(100) && samples.len() < 310 {
                samples.push(json!({"phase":"graphical_running","entered":dir.join("entered.json").exists(),
                    "failure":dir.join("failure.json").exists(),"elapsed_us":start.elapsed().as_micros(),
                    "snapshot":snapshot(kernel(job.accounting_snapshot())?),"messages":kernel(port.drain())?}));
                last_graph = Instant::now();
            }
        } else {
            let receipt = dir.join(format!("{next:03}.json"));
            if receipt.exists() {
                let child: Value = serde_json::from_slice(&fs::read(receipt)?)?;
                samples.push(json!({"phase":"child_barrier","elapsed_us":start.elapsed().as_micros(),"child":child,
                    "snapshot":snapshot(kernel(job.accounting_snapshot())?),"messages":kernel(port.drain())?}));
                fs::write(dir.join(format!("{next:03}.ack")), [])?;
                next += 1;
                if next > 140 {
                    return Err("phase bound".into());
                }
            }
        }
        let deadline = if graphical { 30 } else { 22 };
        if !terminated && start.elapsed() >= Duration::from_secs(deadline) {
            kernel(job.terminate(92))?;
            terminated = true;
            samples.push(json!({"phase":"TerminateJobObject_requested","elapsed_us":start.elapsed().as_micros()}));
        }
        if start.elapsed() > Duration::from_secs(deadline + 5) {
            return Err("original exit unconfirmed; no next process".into());
        }
        thread::sleep(Duration::from_millis(2));
    };
    samples.push(json!({"phase":"original_process_signaled_exit","exit_code":exit,"elapsed_us":start.elapsed().as_micros(),
        "snapshot":snapshot(kernel(job.accounting_snapshot())?),"messages":kernel(port.drain())?}));
    kernel(job.retire())?;
    let objects = job.objects();
    kernel(port.close())?;
    let result = json!({"process_limit":process,"job_limit":aggregate,"step":step,"graphical":graphical,
        "exit_code":exit,"termination_requested":terminated,"elapsed_ms":start.elapsed().as_millis(),
        "retired_objects":{"jobs":objects.jobs,"processes":objects.processes,"threads":objects.threads},
        "diagnostic_port_CloseHandle_succeeded":true,"samples":samples});
    write(&dir.join("accounting.json"), &result)?;
    if !graphical && exit != 0 {
        return Err(format!("CPU child exited {exit}").into());
    }
    Ok(result)
}
fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--child") if args.len() == 4 => child(Path::new(&args[2]), args[3].parse()?),
        Some("--controller") if args.len() == 3 || args.len() == 4 => {
            let dir = Path::new(&args[2]);
            if !dir.is_absolute() { return Err("absolute new output directory required".into()); }
            fs::create_dir(dir)?;
            let exe = if let Some(path) = args.get(3) { Path::new(path).to_path_buf() } else { std::env::current_exe()? };
            let mut results = Vec::new();
            for (name,p,j,step) in [("historical-16-16",16,16,1),("process-16-job-64",16,64,1),
                ("process-64-job-16",64,16,1),("both-64-step-4",64,64,4),("both-64-step-16",64,64,16)] {
                let value = run_case(&exe, &dir.join(name), p*MIB, j*MIB, step*MIB, false)?;
                results.push(json!({"case":name,"exit_code":value["exit_code"],"samples":value["samples"].as_array().unwrap().len()}));
            }
            write(&dir.join("controller.json"), &json!({"controller_in_outer_job":kernel(accounting::controller_in_job())?,
                "architecture":std::env::consts::ARCH,"units":"bytes except explicit counts/times","cases":results}))
        }
        Some("--graphical") if args.len() == 4 => {
            run_case(Path::new(&args[2]), Path::new(&args[3]), 16*MIB,16*MIB,MIB,true)?;
            Ok(())
        }
        _ => Err("usage: --controller NEW_ABSOLUTE_DIR [CPU_CHILD_EXE] | --graphical PROOF_EXE NEW_ABSOLUTE_DIR".into()),
    }
}
