use std::ffi::CString;
use std::fmt::Display;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::exit;

use nix::sys::ptrace;
use nix::sys::signal::kill;
use nix::sys::wait::{WaitStatus, waitpid};
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

impl Display for Pid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_raw())
    }
}

#[derive(Debug, Clone)]
pub enum ProcessState {
    Stopped,
    Running,
    Exited,
    Terminated,
}

pub struct StopReason {
    pub reason: ProcessState,
    pub exit_status: Option<i32>,
    pub signal: Option<String>,
}

impl From<WaitStatus> for StopReason {
    fn from(status: WaitStatus) -> Self {
        match status {
            WaitStatus::Exited(_pid, exit_status) => StopReason {
                reason: ProcessState::Exited,
                exit_status: Some(exit_status),
                signal: None,
            },
            WaitStatus::Signaled(_pid, signal, _core_dump) => StopReason {
                reason: ProcessState::Terminated,
                exit_status: None,
                signal: Some(signal.to_string()),
            },
            WaitStatus::Stopped(_pid, signal) => StopReason {
                reason: ProcessState::Stopped,
                exit_status: None,
                signal: Some(signal.to_string()),
            },
            _ => todo!("Handle other wait statuses"),
        }
    }
}

impl Display for StopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.reason {
            ProcessState::Exited => write!(f, "exited with status: {}", self.exit_status.unwrap()),
            ProcessState::Terminated => {
                write!(
                    f,
                    "terminated with signal: {}",
                    self.signal.as_ref().unwrap()
                )
            }
            ProcessState::Stopped => {
                write!(f, "stopped with signal: {}", self.signal.as_ref().unwrap())
            }
            ProcessState::Running => Ok(()),
        }
    }
}

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("Failed to fork inferior process")]
    Fork,
    #[error("Failed to trace inferior process")]
    Traceme,
    #[error("Failed to exec inferior process")]
    Exec,
    #[error("Can't attach to process with invalid PID")]
    InvalidPid,
    #[error("Failed to attach to process")]
    Attach,
    #[error("Failed to resume inferior process")]
    Resume,
    #[error("Failed waiting for signal on inferior process")]
    Wait,
}

#[derive(Debug)]
pub struct Process {
    pid: Pid,
    terminate_on_end: bool,
    state: ProcessState,
}

impl Process {
    pub fn launch(path: &Path) -> Result<Self, ProcessError> {
        match unsafe { fork().map_err(|_| ProcessError::Fork)? } {
            ForkResult::Parent { child } => {
                let mut process = Self {
                    pid: child.into(),
                    terminate_on_end: true,
                    state: ProcessState::Stopped,
                };
                process.wait_on_signal()?;

                Ok(process)
            }
            ForkResult::Child => {
                ptrace::traceme().map_err(|_| ProcessError::Traceme)?;
                let prog =
                    CString::new(path.as_os_str().as_bytes()).map_err(|_| ProcessError::Exec)?;
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
        if pid.0.as_raw() == 0 {
            return Result::Err(ProcessError::InvalidPid);
        }
        ptrace::attach(pid.0).map_err(|_| ProcessError::Attach)?;
        let mut process = Self {
            pid,
            terminate_on_end: false,
            state: ProcessState::Stopped,
        };
        process.wait_on_signal()?;

        Ok(process)
    }

    pub fn resume(&mut self) -> Result<(), ProcessError> {
        ptrace::cont(self.pid.0, None).map_err(|_| ProcessError::Resume)?;
        self.state = ProcessState::Running;

        Ok(())
    }

    pub fn wait_on_signal(&mut self) -> Result<StopReason, ProcessError> {
        let wait_status = waitpid(self.pid.0, None).map_err(|_| ProcessError::Wait)?;
        let reason: StopReason = wait_status.into();
        self.state = reason.reason.clone();

        Ok(reason)
    }

    pub fn pid(&self) -> Pid {
        self.pid
    }
}

#[allow(unused_must_use)]
impl Drop for Process {
    fn drop(&mut self) {
        if self.pid.0.as_raw() == 0 {
            return;
        }

        if let ProcessState::Running = self.state {
            kill(self.pid.0, nix::sys::signal::Signal::SIGSTOP);
            waitpid(self.pid.0, None);
        }
        ptrace::detach(self.pid.0, None);
        kill(self.pid.0, nix::sys::signal::Signal::SIGCONT);

        if self.terminate_on_end {
            kill(self.pid.0, nix::sys::signal::Signal::SIGKILL);
            waitpid(self.pid.0, None);
        }
    }
}
