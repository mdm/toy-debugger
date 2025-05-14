use std::{error::Error, path::PathBuf};

use clap::Parser;

use reedline::{DefaultPrompt, DefaultPromptSegment, FileBackedHistory, Reedline, Signal};
use toy_debugger::process::{Pid, Process, ProcessState, StopReason};

#[derive(Parser)]
struct Cli {
    #[arg(short, conflicts_with = "path")]
    pid: Option<i32>,
    path: Option<PathBuf>,
}

fn handle_command(process: &mut Process, line: &str) -> Result<(), Box<dyn Error>> {
    let mut args = line.split_whitespace();
    let command = args.next().unwrap_or_default();

    if "continue".starts_with(command) {
        process.resume()?;
        let stop_reason = process.wait_on_signal()?;
        print_stop_reason(process, &stop_reason);
    } else {
        eprintln!("Unknown command");
    }

    Ok(())
}

fn print_stop_reason(process: &Process, reason: &StopReason) {
    if let ProcessState::Running = reason.reason {
        return;
    }

    println!("Process {} {}", process.pid(), reason);
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    let mut process = match (cli.pid, cli.path) {
        (Some(pid), None) => Process::attach(Pid::from(pid))?,
        (None, Some(path)) => Process::launch(&path)?,
        _ => unreachable!(),
    };

    let history = Box::new(
        FileBackedHistory::with_file(8, "history.txt".into()).expect("Error configuring history"),
    );
    let mut line_editor = Reedline::create().with_history(history);
    let prompt = DefaultPrompt::new(
        DefaultPromptSegment::Basic("td".to_string()),
        DefaultPromptSegment::Empty,
    );
    loop {
        let signal = line_editor.read_line(&prompt);
        match signal {
            Ok(Signal::Success(buffer)) => {
                handle_command(&mut process, &buffer).unwrap_or_else(|e| {
                    eprintln!("Error handling command: {}", e);
                });
            }
            Ok(Signal::CtrlD) | Ok(Signal::CtrlC) => {
                break;
            }
            x => {
                eprintln!("Unhandled Reedline event: {:?}", x);
            }
        }
    }

    Ok(())
}
