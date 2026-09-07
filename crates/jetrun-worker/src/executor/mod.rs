pub mod docker;
pub mod native;

/// Output line from a step execution
#[derive(Debug, Clone)]
pub struct ExecOutput {
    pub stream: ExecStream,
    pub content: String,
}

#[derive(Debug, Clone, Copy)]
pub enum ExecStream {
    Stdout,
    Stderr,
}

/// Result of a completed step execution
#[derive(Debug)]
pub struct ExecResult {
    pub exit_code: i32,
    pub duration_ms: u64,
}
