//! No-GPU fixture binary, also a parent-death controller using the real seam.
#![deny(unsafe_code)]
#[path = "../../apps/lab-workbench/src/supervisor/mod.rs"]
mod supervisor;
use std::{
    io::{Read, Write},
    path::Path,
    time::{Duration, Instant},
};
use supervisor::{Backend, Launch, Phase, Policy, Progress, Supervisor, WindowsBackend};

struct ReportingBackend {
    inner: WindowsBackend,
    directory: std::path::PathBuf,
}
impl Backend for ReportingBackend {
    fn prepare(&mut self, launch: &Launch, incarnation: u64) -> Result<(), lab_windows_job::Error> {
        self.inner.prepare(launch, incarnation)?;
        let (pid, original) = self
            .inner
            .job
            .as_ref()
            .unwrap()
            .fixture_process_identity()?;
        std::fs::write(
            self.directory.join("identity"),
            format!("{pid} {original}\n"),
        )
        .map_err(|e| lab_windows_job::Error {
            operation: "fixture identity write",
            code: e.raw_os_error().unwrap_or(0) as u32,
        })
    }
    fn resume(&mut self) -> Result<(), lab_windows_job::Error> {
        self.inner.resume()
    }
    fn has_process(&self) -> bool {
        self.inner.has_process()
    }
    fn poll_exit(&mut self) -> Result<Option<u32>, lab_windows_job::Error> {
        self.inner.poll_exit()
    }
    fn challenge(&mut self, i: u64, s: u64) -> Result<(), lab_windows_job::Error> {
        self.inner.challenge(i, s)
    }
    fn progress(&mut self) -> Result<Option<Progress>, lab_windows_job::Error> {
        self.inner.progress()
    }
    fn close(&mut self, i: u64, s: u64) -> Result<(), lab_windows_job::Error> {
        self.inner.close(i, s)
    }
    fn terminate(&mut self, r: bool) -> Result<(), lab_windows_job::Error> {
        self.inner.terminate(r)
    }
    fn retire(&mut self) -> Result<lab_windows_job::Reading, lab_windows_job::Error> {
        self.inner.retire()
    }
    fn objects(&self) -> lab_windows_job::Objects {
        self.inner.objects()
    }
}

fn packet(dir: &Path) -> Option<(u64, u64, char)> {
    let file = std::fs::File::open(dir.join("challenge")).ok()?;
    let mut bytes = Vec::with_capacity(65);
    file.take(65).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 64 || !bytes.ends_with(b"\n") {
        return None;
    }
    let text = std::str::from_utf8(&bytes).ok()?;
    let mut fields = text.split_whitespace();
    let incarnation = fields.next()?.parse().ok()?;
    let sequence = fields.next()?.parse().ok()?;
    let kind = match fields.next()? {
        "C" => 'C',
        "S" => 'S',
        "I" => 'I',
        _ => return None,
    };
    if fields.next().is_some() {
        return None;
    }
    Some((incarnation, sequence, kind))
}
fn ack(dir: &Path, incarnation: u64, sequence: u64, counter: u64, healthy: bool) {
    let payload = format!(
        "{incarnation} {sequence} {counter} {}\n",
        if healthy { "H" } else { "P" }
    );
    assert!(payload.len() <= 96);
    let mut file = std::fs::File::create(dir.join("ack")).unwrap();
    file.write_all(payload.as_bytes()).unwrap();
}
fn child(mode: &str, dir: &Path) {
    std::fs::write(dir.join("executed"), "owner entered\n").unwrap();
    if mode == "descendant" {
        let attempt = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--loop", "hang", dir.to_str().unwrap()])
            .spawn();
        match attempt {
            Err(error) => {
                let code = error.raw_os_error().unwrap();
                std::fs::write(dir.join("descendant-denied"), format!("{code}\n")).unwrap();
                assert_eq!(code, 1816);
                std::process::exit(91);
            }
            Ok(mut unexpected) => {
                unexpected.kill().unwrap();
                unexpected.wait().unwrap();
                panic!("active-process limit admitted a descendant");
            }
        }
    }
    if mode == "commit" {
        let denied = lab_windows_job::commit_fixture().unwrap();
        std::fs::write(dir.join("memory"), format!("denied={denied}\n")).unwrap();
        std::process::exit(90);
    }
    if mode == "hang" {
        loop {
            std::thread::park();
        }
    }
    let mut sequence = 0;
    let mut counter = 0_u64;
    loop {
        if let Some((incarnation, next, kind)) = packet(dir) {
            if kind == 'S' {
                counter = counter.checked_add(1).unwrap();
                ack(dir, incarnation, next, counter, true);
                return;
            }
            if kind == 'C' && next > sequence {
                sequence = next;
                counter = counter.checked_add(1).unwrap();
                ack(dir, incarnation, next, counter, false);
                if mode == "crash" {
                    std::process::exit(31);
                }
                if mode == "helper" {
                    let directory = dir.to_owned();
                    std::thread::spawn(move || {
                        loop {
                            if let Some((inc, seq, _)) = packet(&directory) {
                                ack(&directory, inc, seq, counter, false);
                            }
                            std::thread::park_timeout(Duration::from_millis(5));
                        }
                    });
                    loop {
                        std::thread::park();
                    } // Owner stops; helper can still pong.
                }
            }
        }
        // This cadence is not progress evidence: only exact challenge/counter is.
        std::thread::park_timeout(Duration::from_millis(5));
    }
}
fn owner(dir: &Path) {
    let exe = std::env::current_exe().unwrap();
    let directory = dir.to_owned();
    let supervisor = Supervisor::spawn(
        move || ReportingBackend {
            inner: WindowsBackend::default(),
            directory,
        },
        Policy::default(),
    )
    .unwrap();
    supervisor
        .restart(Launch {
            executable: exe,
            arguments: vec![
                "--loop".into(),
                "normal".into(),
                dir.to_str().unwrap().into(),
            ],
            fixture_directory: dir.to_owned(),
            limits: lab_windows_job::Limits {
                process_commit: 32 * 1024 * 1024,
                job_commit: 32 * 1024 * 1024,
            },
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(status) = supervisor.status()
            && status.incarnation != 0
            && status.phase == Phase::Stopped
            && let Some(error) = status.original_error
        {
            panic!("fixture startup rejected: {error:?}");
        }
        if supervisor.status().is_ok_and(|s| s.phase == Phase::Ready) {
            std::fs::write(dir.join("owner-ready"), "ready\n").unwrap();
            break;
        }
        assert!(Instant::now() < deadline, "fixture supervisor startup");
        std::thread::park_timeout(Duration::from_millis(5));
    }
    loop {
        std::thread::park();
    } // External controller kills this parent process.
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--loop") => child(&args[2], Path::new(&args[3])),
        Some("--owner") => owner(Path::new(&args[2])),
        _ => panic!("closed no-GPU fixture roles only"),
    }
}
