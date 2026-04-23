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

/// Strip ANSI escape sequences for parsing. Keeps original text untouched.
fn strip_ansi(s: &str) -> String {
    // Match CSI sequences: ESC [ ... letter
    let re = Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap();
    re.replace_all(s, "").to_string()
}

/// Parse gcc diagnostic lines from stderr.
///
/// Format:
///   FILE:LINE:COL: level: message [CODE]
///       LINE |     source line
///            |                  ^~~~~
///
/// Level: error | warning | note | fatal error
/// Extract the end column from a caret-annotation line like "      |         ^~~~~".
/// Returns the number of characters after `^` (including `^` itself minus 1).
fn extract_tilde_count(caret_line: &str) -> Option<usize> {
    let clean = strip_ansi(caret_line);
    let trimmed = clean.trim_start();
    // Expect "| " prefix then spaces then ^ then ~~~.
    let after_bar = trimmed.strip_prefix('|')?;
    let caret_pos = after_bar.find('^')?;
    let after_caret = &after_bar[caret_pos + 1..];
    let tilde_count = after_caret.chars().take_while(|&c| c == '~').count();
    Some(tilde_count)
}

fn parse_gcc_diagnostics(stderr: &str) -> Vec<Diagnostic> {
    let main_re = Regex::new(
        r"^(?P<file>[^:\n]+):(?P<line>\d+):(?P<col>\d+): (?P<level>error|warning|note|fatal error): (?P<msg>.+)$",
    )
    .unwrap();
    let code_re = Regex::new(r"\s*\[([^\]]+)\]$").unwrap();

    let lines: Vec<&str> = stderr.lines().collect();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let clean = strip_ansi(line);
        let caps = match main_re.captures(&clean) {
            Some(c) => c,
            None => continue,
        };
        // Skip diagnostics from system headers / stdlib.
        let file = &caps["file"];
        if file.starts_with('/') || file.contains("/include/") {
            continue;
        }
        let line_num: usize = caps["line"].parse().unwrap_or(1);
        let col_num: usize = caps["col"].parse().unwrap_or(1);
        let mut level = caps["level"].to_string();
        if level == "fatal error" {
            level = "error".to_string();
        }
        let mut message = caps["msg"].to_string();
        let code = code_re.captures(&message).map(|c| c[1].to_string());
        if code.is_some() {
            message = code_re.replace(&message, "").to_string();
        }
        // Look ahead 1-3 lines for a caret annotation to determine end column.
        // gcc prints: header / source-line / "      |    ^~~~"
        let end_column = lines[i + 1..(i + 4).min(lines.len())]
            .iter()
            .find_map(|l| extract_tilde_count(l))
            .map(|tildes| col_num + tildes); // tildes after ^, so end = start + 1 + tildes, but col is 1-based inclusive — give end past the last ~.
        let span = Some(Span {
            start: Position {
                line: line_num.saturating_sub(1),
                column: col_num.saturating_sub(1),
            },
            end: end_column.map(|c| Position {
                line: line_num.saturating_sub(1),
                column: c,
            }),
        });
        let diag = Diagnostic {
            level: level.clone(),
            span,
            message,
            code,
            details: Vec::new(),
        };
        if level == "note" && !diagnostics.is_empty() {
            let last_idx = diagnostics.len() - 1;
            if diagnostics[last_idx].level != "note" {
                diagnostics[last_idx].details.push(diag);
                continue;
            }
        }
        diagnostics.push(diag);
    }
    diagnostics
}

fn try_main() -> Result<i32, (i32, String)> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err((
            1,
            "usage: judge-gcc-compile <diagnostics-file> <gcc args...>".to_string(),
        ));
    }
    let diagnostics_path = Path::new(&args[1]).to_path_buf();
    let gcc_args: Vec<&str> = args[2..].iter().map(|s| s.as_str()).collect();

    // Determine compiler: first arg that isn't a flag is the compiler name.
    let compiler = gcc_args.first().copied().unwrap_or("gcc");
    let compiler_args = &gcc_args[1..];

    let mut child = Command::new(compiler)
        .args(compiler_args)
        .arg("-fdiagnostics-color=always")
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
    let diagnostics = parse_gcc_diagnostics(&captured_text);
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
