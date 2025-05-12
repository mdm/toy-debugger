use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::exit;

use nix::sys::ptrace;
use nix::unistd::{ForkResult, execvp, fork};
use thiserror::Error;

#[derive(Debug, Clone, Copy)]
pub struct Pid(nix::unistd::Pid);

impl From<nix::unistd::Pid> for Pid {
    fn from(pid: nix::unistd::Pid) -> Self {
        Pid(pid)
    }
}

impl From<i32> for Pid {
    fn from(pid: i32) -> Self {
        Pid(nix::unistd::Pid::from_raw(pid))
    }
}

#[derive(Debug)]
pub enum ProcessState {
    Stopped,
    Running,
    Exited,
    Terminated,
}

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("Failed to fork inferior process")]
    ForkError,
    #[error("Failed to trace inferior process")]
    TracemeError,
    #[error("Failed to exec inferior process")]
    ExecError,
    #[error("Failed to attach to process")]
    AttachError,
}

#[derive(Debug)]
pub struct Process {
    pid: Pid,
    terminate_on_end: bool,
    state: ProcessState,
}

impl Process {
    pub fn launch(path: &Path) -> Result<Self, ProcessError> {
        match unsafe { fork().map_err(|_| ProcessError::ForkError)? } {
            ForkResult::Parent { child } => {
                let process = Self {
                    pid: child.into(),
                    terminate_on_end: false,
                    state: ProcessState::Stopped,
                };
                process.wait_on_signal();
                Ok(process)
            }
            ForkResult::Child => {
                ptrace::traceme().map_err(|_| ProcessError::TracemeError)?;
                let prog = CString::new(path.as_os_str().as_bytes())
                    .map_err(|_| ProcessError::ExecError)?;
                let args = [prog.clone()];
                match execvp(&prog, &args) {
                    Ok(_) => unreachable!(),
                    Err(_) => {
                        eprintln!("Failed to exec process: {}", path.display());
                        exit(1);
                    }
                }
            }
        }
    }

    pub fn attach(pid: Pid) -> Result<Self, ProcessError> {
        ptrace::attach(pid.0).map_err(|_| ProcessError::AttachError)?;
        let process = Self {
            pid,
            terminate_on_end: false,
            state: ProcessState::Stopped,
        };

        Ok(process)
    }

    pub fn resume(&self) {}

    pub fn wait_on_signal(&self) {}

    pub fn pid(&self) -> Pid {
        self.pid
    }
}
