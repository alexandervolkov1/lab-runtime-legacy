//! Explicit experimental stock host selection; no deployment memory default.
use std::path::PathBuf;
#[derive(Debug)]
pub(crate) struct ExperimentalGui {
    pub(crate) renderer: String,
    pub(crate) executable: PathBuf,
    pub(crate) commit_limit: usize,
    pub(crate) restart_limit: u8,
    pub(crate) capture: bool,
}
#[derive(Default)]
pub(crate) struct Candidate {
    renderer: Option<String>,
    executable: Option<PathBuf>,
    commit_limit: Option<usize>,
    restart_limit: Option<u8>,
    capture: bool,
}
impl Candidate {
    pub(crate) fn argument(
        &mut self,
        arg: &str,
        args: &mut impl Iterator<Item = String>,
    ) -> Result<bool, &'static str> {
        match arg {
            "--experimental-eframe" if self.renderer.is_none() => {
                let renderer = args.next().ok_or("explicit eframe renderer required")?;
                if !matches!(renderer.as_str(), "wgpu" | "glow") {
                    return Err("eframe renderer must be wgpu or glow");
                }
                self.renderer = Some(renderer);
            }
            "--experimental-eframe-child" if self.executable.is_none() => {
                self.executable = Some(PathBuf::from(
                    args.next().ok_or("eframe child path required")?,
                ))
            }
            "--experimental-eframe-commit-limit" if self.commit_limit.is_none() => {
                let limit = args
                    .next()
                    .ok_or("experimental commit limit required")?
                    .parse::<usize>()
                    .map_err(|_| "invalid experimental commit limit")?;
                if limit == 0 {
                    return Err("experimental commit limit must be positive");
                }
                self.commit_limit = Some(limit);
            }
            "--experimental-eframe-restart-limit" if self.restart_limit.is_none() => {
                let limit = args
                    .next()
                    .ok_or("GUI restart limit required")?
                    .parse::<u8>()
                    .map_err(|_| "invalid GUI restart limit")?;
                if limit > 3 {
                    return Err("experimental restart limit exceeds three");
                }
                self.restart_limit = Some(limit);
            }
            "--experimental-eframe-capture" if !self.capture => self.capture = true,
            _ => return Ok(false),
        }
        Ok(true)
    }
    pub(crate) fn finish(self) -> Result<Option<ExperimentalGui>, &'static str> {
        let Some(renderer) = self.renderer else {
            if self.executable.is_some()
                || self.commit_limit.is_some()
                || self.restart_limit.is_some()
                || self.capture
            {
                return Err("eframe options require explicit experimental opt-in");
            }
            return Ok(None);
        };
        let executable = self
            .executable
            .ok_or("explicit eframe child path required")?;
        if !executable.is_absolute() {
            return Err("eframe child path must be absolute");
        }
        Ok(Some(ExperimentalGui {
            renderer,
            executable,
            commit_limit: self
                .commit_limit
                .ok_or("explicit experimental commit limit required; no deployment default")?,
            restart_limit: self.restart_limit.unwrap_or(0),
            capture: self.capture,
        }))
    }
}

#[cfg(test)]
mod tests {
    use crate::arguments_from;
    fn args(tail: &[&str]) -> Result<super::ExperimentalGui, &'static str> {
        let mut values = vec!["--connect".to_owned(), "127.0.0.1:7420".to_owned()];
        values.extend(tail.iter().map(|s| (*s).to_owned()));
        arguments_from(values)?
            .experimental
            .ok_or("no experimental opt-in")
    }
    #[test]
    fn opt_in_requires_renderer_executable_and_explicit_memory_without_fallback() {
        assert!(args(&["--experimental-eframe", "wgpu"]).is_err());
        assert!(args(&["--experimental-eframe-commit-limit", "1073741824"]).is_err());
        let base = [
            "--experimental-eframe",
            "wgpu",
            "--experimental-eframe-child",
            r"D:\lab\lab-eframe-host.exe",
            "--experimental-eframe-commit-limit",
            "1073741824",
        ];
        let options = args(&base).unwrap();
        assert_eq!(options.renderer, "wgpu");
        assert_eq!(options.restart_limit, 0);
        for bad in ["auto", "custom-glow", ""] {
            let mut changed = base;
            changed[1] = bad;
            assert!(args(&changed).is_err());
        }
        let mut changed = base;
        changed[5] = "0";
        assert!(args(&changed).is_err());
        let mut extra = base.to_vec();
        extra.extend(["--experimental-eframe-restart-limit", "4"]);
        assert!(args(&extra).is_err());
        assert!(
            arguments_from(["--connect".into(), "127.0.0.1:7420".into()])
                .unwrap()
                .experimental
                .is_none()
        );
    }
}
