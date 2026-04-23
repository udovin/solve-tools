use regex::Regex;
use serde::Serialize;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

#[derive(Serialize)]
struct Position {
    line: usize,
    column: usize,
}

#[derive(Serialize)]
struct Span {
    start: Position,
    #[serde(skip_serializing_if = "Option::is_none")]
    end: Option<Position>,
}

#[derive(Serialize)]
struct Diagnostic {
    level: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    span: Option<Span>,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    details: Vec<Diagnostic>,
}

/// Parse fpc diagnostic lines from stdout.
///
/// Format:
///   FILE(LINE,COL) Fatal|Error|Warning|Note|Hint: message
///   FILE(LINE,COL) Fatal|Error|Warning|Note|Hint: (CODE) message
///
/// Also skip standalone "Fatal: Compilation aborted" lines.
fn parse_fpc_diagnostics(output: &str) -> Vec<Diagnostic> {
    let main_re =
        Regex::new(r"^(.+?)\((\d+),(\d+)\)\s+(Fatal|Error|Warning|Note|Hint):\s+(.+)$").unwrap();
    let code_re = Regex::new(r"^\((\d+)\)\s+(.+)$").unwrap();

    let mut diagnostics = Vec::new();
    for line in output.lines() {
        let caps = match main_re.captures(line) {
            Some(c) => c,
            None => continue,
        };
        let file = &caps[1];
        if file.starts_with('/') || file.contains("/include/") {
            continue;
        }
        let line_num: usize = caps[2].parse().unwrap_or(1);
        let col_num: usize = caps[3].parse().unwrap_or(1);
        let level = match &caps[4] {
            "Fatal" | "Error" => "error",
            "Warning" => "warning",
            "Note" => "note",
            "Hint" => "help",
            _ => "note",
        }
        .to_string();
        let raw_message = caps[5].to_string();

        // Try to extract error code from message: (5002) actual message
        let (code, message) = match code_re.captures(&raw_message) {
            Some(c) => (Some(c[1].to_string()), c[2].to_string()),
            None => (None, raw_message),
        };

        diagnostics.push(Diagnostic {
            level,
            span: Some(Span {
                start: Position {
                    line: line_num.saturating_sub(1),
                    column: col_num.saturating_sub(1),
                },
                end: None,
            }),
            message,
            code,
            details: Vec::new(),
        });
    }
    diagnostics
}

fn try_main() -> Result<i32, (i32, String)> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err((
            1,
            "usage: judge-fpc-compile <diagnostics-file> <fpc args...>".to_string(),
        ));
    }
    let diagnostics_path = Path::new(&args[1]).to_path_buf();
    let compiler = &args[2];
    let compiler_args: Vec<&str> = args[3..].iter().map(|s| s.as_str()).collect();

    // fpc outputs diagnostics to stdout, so we capture stdout and forward to stderr.
    let mut child = Command::new(compiler)
        .args(&compiler_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| (1, format!("cannot run {}: {}", compiler, e)))?;

    // Tee stdout (fpc diagnostics) to stderr, collect for parsing.
    let mut captured = Vec::new();
    if let Some(stdout_pipe) = child.stdout.take() {
        let reader = BufReader::new(stdout_pipe);
        let mut err = io::stderr();
        for chunk in reader.split(b'\n') {
            let mut chunk = chunk.map_err(|e| (1, format!("cannot read stdout: {}", e)))?;
            err.write_all(&chunk).ok();
            err.write_all(b"\n").ok();
            chunk.push(b'\n');
            captured.extend_from_slice(&chunk);
        }
    }
    // Also forward stderr.
    if let Some(stderr_pipe) = child.stderr.take() {
        let reader = BufReader::new(stderr_pipe);
        let mut err = io::stderr();
        for chunk in reader.split(b'\n') {
            let chunk = chunk.map_err(|e| (1, format!("cannot read stderr: {}", e)))?;
            err.write_all(&chunk).ok();
            err.write_all(b"\n").ok();
        }
    }
    let captured_text = String::from_utf8_lossy(&captured).to_string();

    let status = child
        .wait()
        .map_err(|e| (1, format!("cannot wait {}: {}", compiler, e)))?;

    // Write NDJSON diagnostics.
    let diagnostics = parse_fpc_diagnostics(&captured_text);
    if let Ok(mut file) = fs::File::create(&diagnostics_path) {
        for d in &diagnostics {
            if let Ok(json) = serde_json::to_string(d) {
                let _ = writeln!(file, "{}", json);
            }
        }
    }

    Ok(status.code().unwrap_or(1))
}

fn main() -> ExitCode {
    match try_main() {
        Ok(code) => ExitCode::from(code as u8),
        Err((code, msg)) => {
            if !msg.is_empty() {
                eprintln!("{}", msg);
            }
            ExitCode::from(code as u8)
        }
    }
}
