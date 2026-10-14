//! Safe OS worker adapter. File challenges are a bounded NO-GPU fixture seam,
//! not production IPC or snapshots. Only SystemRoot is inherited as environment.
use super::{Backend, Progress};
use lab_windows_job::{Error, Job, Limits, Objects, Reading};
use std::{
    io::{Read, Write},
    path::PathBuf,
};

#[derive(Debug)]
pub(crate) struct Launch {
    pub(crate) executable: PathBuf,
    pub(crate) arguments: Vec<String>,
    pub(crate) limits: Limits,
    pub(crate) fixture_directory: PathBuf,
}
impl Launch {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        self.limits.validate()?;
        if self.arguments.len() > 8
            || self.arguments.capacity() > 8
            || self.arguments.iter().any(|s| s.capacity() > 4096)
            || self.executable.capacity() > 1024
            || self.fixture_directory.capacity() > 1024
        {
            return Err(Error {
                operation: "argument count bound",
                code: 0,
            });
        }
        let args: Vec<_> = self.arguments.iter().map(String::as_str).collect();
        lab_windows_job::validate_launch(&self.executable, &args)?;
        if !self.fixture_directory.is_absolute() || self.fixture_directory.as_os_str().len() > 1024
        {
            return Err(Error {
                operation: "fixture directory bounds",
                code: 0,
            });
        }
        Ok(())
    }
}
#[derive(Default)]
pub(crate) struct WindowsBackend {
    pub(crate) job: Option<Job>,
    directory: Option<PathBuf>,
}
fn io_error(e: std::io::Error, operation: &'static str) -> Error {
    Error {
        operation,
        code: e.raw_os_error().unwrap_or(0) as u32,
    }
}
impl WindowsBackend {
    fn command(&self, incarnation: u64, sequence: u64, kind: char) -> Result<(), Error> {
        let dir = self.directory.as_ref().ok_or(Error {
            operation: "missing fixture channel",
            code: 0,
        })?;
        let payload = format!("{incarnation} {sequence} {kind}\n");
        debug_assert!(payload.len() <= 64);
        let mut file = std::fs::File::create(dir.join("challenge"))
            .map_err(|e| io_error(e, "fixture command open"))?;
        file.write_all(payload.as_bytes())
            .map_err(|e| io_error(e, "fixture command write"))
    }
}
impl Backend for WindowsBackend {
    fn prepare(&mut self, launch: &Launch, _: u64) -> Result<(), Error> {
        launch.validate()?;
        if self.job.is_some() {
            return Err(Error {
                operation: "old Job obligation",
                code: 0,
            });
        }
        self.job = Some(Job::new(launch.limits)?);
        self.directory = Some(launch.fixture_directory.clone());
        self.command(0, 0, 'I')?;
        let args: Vec<_> = launch.arguments.iter().map(String::as_str).collect();
        self.job
            .as_mut()
            .unwrap()
            .prepare(&launch.executable, &args)
    }
    fn resume(&mut self) -> Result<(), Error> {
        self.job.as_mut().unwrap().resume()
    }
    fn has_process(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|j| j.objects().processes != 0)
    }
    fn poll_exit(&mut self) -> Result<Option<u32>, Error> {
        self.job.as_ref().unwrap().poll_exit()
    }
    fn challenge(&mut self, incarnation: u64, sequence: u64) -> Result<(), Error> {
        self.command(incarnation, sequence, 'C')
    }
    fn progress(&mut self) -> Result<Option<Progress>, Error> {
        let Some(dir) = &self.directory else {
            return Ok(None);
        };
        let file = match std::fs::File::open(dir.join("ack")) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_error(e, "fixture ack open")),
        };
        let mut bytes = Vec::with_capacity(97);
        file.take(97)
            .read_to_end(&mut bytes)
            .map_err(|e| io_error(e, "fixture ack read"))?;
        if bytes.len() > 96 {
            return Err(Error {
                operation: "fixture ack overflow",
                code: 0,
            });
        }
        if !bytes.ends_with(b"\n") {
            return Ok(None);
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            return Err(Error {
                operation: "fixture ack UTF-8",
                code: 0,
            });
        };
        let fields: Vec<_> = text.split_whitespace().collect();
        if fields.len() != 4 {
            return Err(Error {
                operation: "fixture ack fields",
                code: 0,
            });
        }
        let parse = |s: &str| {
            s.parse::<u64>().map_err(|_| Error {
                operation: "fixture ack counter",
                code: 0,
            })
        };
        let healthy_close = match fields[3] {
            "P" => false,
            "H" => true,
            _ => {
                return Err(Error {
                    operation: "fixture ack variant",
                    code: 0,
                });
            }
        };
        Ok(Some(Progress {
            incarnation: parse(fields[0])?,
            challenge: parse(fields[1])?,
            owner_counter: parse(fields[2])?,
            healthy_close,
        }))
    }
    fn close(&mut self, incarnation: u64, sequence: u64) -> Result<(), Error> {
        self.command(incarnation, sequence, 'S')
    }
    fn terminate(&mut self, rollback: bool) -> Result<(), Error> {
        let job = self.job.as_ref().unwrap();
        if rollback {
            job.rollback(77)
        } else {
            job.terminate(77)
        }
    }
    fn retire(&mut self) -> Result<Reading, Error> {
        let mut reading = Reading::default();
        if let Some(job) = &mut self.job {
            reading = job.verify_limits()?;
            job.retire()?;
        }
        self.job = None;
        self.directory = None;
        Ok(reading)
    }
    fn objects(&self) -> Objects {
        self.job.as_ref().map_or(Objects::default(), Job::objects)
    }
}
