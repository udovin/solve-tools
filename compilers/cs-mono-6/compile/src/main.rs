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

/// Parse mcs diagnostic lines from stderr.
///
/// Format:
///   FILE(LINE,COL): error|warning CSxxxx: message
fn parse_mcs_diagnostics(stderr: &str) -> Vec<Diagnostic> {
    let main_re =
        Regex::new(r"^(.+?)\((\d+),?(\d+)?\):\s+(error|warning)\s+(CS\d+):\s+(.+)$").unwrap();

    let mut diagnostics = Vec::new();
    for line in stderr.lines() {
        let caps = match main_re.captures(line) {
            Some(c) => c,
            None => continue,
        };
        let file = &caps[1];
        if file.starts_with('/') || file.contains("/include/") {
            continue;
        }
        let line_num: usize = caps[2].parse().unwrap_or(1);
        let col_num: usize = caps
            .get(3)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);
        let level = caps[4].to_string();
        let code = caps[5].to_string();
        let message = caps[6].to_string();

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
            code: Some(code),
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
            "usage: judge-mcs-compile <diagnostics-file> <mcs args...>".to_string(),
        ));
    }
    let diagnostics_path = Path::new(&args[1]).to_path_buf();
    let compiler = &args[2];
    let compiler_args: Vec<&str> = args[3..].iter().map(|s| s.as_str()).collect();

    let mut child = Command::new(compiler)
        .args(&compiler_args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| (1, format!("cannot run {}: {}", compiler, e)))?;

    // Tee stderr: stream to real stderr, collect for parsing.
    let mut captured = Vec::new();
    if let Some(stderr_pipe) = child.stderr.take() {
        let reader = BufReader::new(stderr_pipe);
        let mut err = io::stderr();
        for chunk in reader.split(b'\n') {
            let mut chunk = chunk.map_err(|e| (1, format!("cannot read stderr: {}", e)))?;
            err.write_all(&chunk).ok();
            err.write_all(b"\n").ok();
            chunk.push(b'\n');
            captured.extend_from_slice(&chunk);
        }
    }
    let captured_text = String::from_utf8_lossy(&captured).to_string();

    let status = child
        .wait()
        .map_err(|e| (1, format!("cannot wait {}: {}", compiler, e)))?;

    // Write NDJSON diagnostics.
    let diagnostics = parse_mcs_diagnostics(&captured_text);
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
