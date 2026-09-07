use chrono::Utc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::executor::{ExecOutput, ExecStream};
use jetrun_common::models::{BuildLog, LogStream};

/// Collects output from an executor and formats it into structured log entries.
pub struct LogCollector {
    step_id: Uuid,
    line_number: u64,
}

impl LogCollector {
    pub fn new(step_id: Uuid) -> Self {
        Self {
            step_id,
            line_number: 0,
        }
    }

    /// Convert executor output into a structured BuildLog entry.
    pub fn format(&mut self, output: &ExecOutput) -> BuildLog {
        self.line_number += 1;
        BuildLog {
            step_id: self.step_id,
            line_number: self.line_number,
            timestamp: Utc::now(),
            stream: match output.stream {
                ExecStream::Stdout => LogStream::Stdout,
                ExecStream::Stderr => LogStream::Stderr,
            },
            content: output.content.clone(),
        }
    }

    /// Spawn a task that reads from executor output and forwards structured logs.
    pub fn spawn_collector(
        step_id: Uuid,
        mut exec_rx: mpsc::Receiver<ExecOutput>,
        log_tx: mpsc::Sender<BuildLog>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut collector = LogCollector::new(step_id);
            while let Some(output) = exec_rx.recv().await {
                let log = collector.format(&output);
                if log_tx.send(log).await.is_err() {
                    break;
                }
            }
        })
    }
}
