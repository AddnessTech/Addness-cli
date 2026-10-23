//! Each child owns an output channel. EOF follows the last line from each reader,
//! so process exit alone cannot finish a turn while output is still in flight.

use std::io::{BufRead, BufReader, Read};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

pub(super) enum OutputEvent {
    Stdout(String),
    Stderr(String),
    Closed,
}

pub(super) struct ProcessOutput {
    rx: Receiver<OutputEvent>,
    open_readers: usize,
}

impl ProcessOutput {
    pub(super) fn new(
        stdout: impl Read + Send + 'static,
        stderr: impl Read + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        spawn_line_reader(stdout, tx.clone(), false);
        spawn_line_reader(stderr, tx, true);
        Self {
            rx,
            open_readers: 2,
        }
    }

    pub(super) fn try_recv(&mut self) -> Option<OutputEvent> {
        match self.rx.try_recv() {
            Ok(OutputEvent::Closed) => {
                self.open_readers = self.open_readers.saturating_sub(1);
                Some(OutputEvent::Closed)
            }
            Ok(event) => Some(event),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.open_readers = 0;
                None
            }
        }
    }

    pub(super) fn is_drained(&self) -> bool {
        self.open_readers == 0
    }
}

fn spawn_line_reader(reader: impl Read + Send + 'static, tx: Sender<OutputEvent>, stderr: bool) {
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            let line = match line {
                Ok(line) => line,
                Err(error) => {
                    let stream = if stderr { "stderr" } else { "stdout" };
                    let _ = tx.send(OutputEvent::Stderr(format!(
                        "{stream} の読み取りに失敗しました: {error}"
                    )));
                    break;
                }
            };
            let event = if stderr {
                OutputEvent::Stderr(line)
            } else {
                OutputEvent::Stdout(line)
            };
            if tx.send(event).is_err() {
                return;
            }
        }
        let _ = tx.send(OutputEvent::Closed);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::time::{Duration, Instant};

    #[test]
    fn drains_large_stdout_and_stderr_including_unterminated_final_line() {
        let stdout = format!("{}final result", "event\n".repeat(20_000));
        let mut output = ProcessOutput::new(Cursor::new(stdout), Cursor::new("last error"));
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut lines = Vec::new();
        let mut errors = Vec::new();
        while !output.is_drained() {
            assert!(Instant::now() < deadline, "readers did not reach EOF");
            match output.try_recv() {
                Some(OutputEvent::Stdout(line)) => lines.push(line),
                Some(OutputEvent::Stderr(line)) => errors.push(line),
                Some(OutputEvent::Closed) => {}
                None => std::thread::yield_now(),
            }
        }
        assert_eq!(lines.len(), 20_001);
        assert_eq!(lines.last().unwrap(), "final result");
        assert_eq!(errors, ["last error"]);
    }
}
