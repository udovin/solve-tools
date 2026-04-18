use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

// --- Output format (our unified schema) ---

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

// --- rustc JSON format (minimal subset we need) ---

#[derive(Deserialize)]
struct RustcMessage {
    message: String,
    #[serde(default)]
    code: Option<RustcCode>,
    level: String,
    #[serde(default)]
    spans: Vec<RustcSpan>,
    #[serde(default)]
    children: Vec<RustcMessage>,
    #[serde(default)]
    rendered: Option<String>,
}

#[derive(Deserialize)]
struct RustcCode {
    code: String,
}

#[derive(Deserialize)]
struct RustcSpan {
    line_start: usize,
    line_end: usize,
    column_start: usize,
    column_end: usize,
    is_primary: bool,
    #[serde(default)]
    label: Option<String>,
}

fn pick_primary(spans: &[RustcSpan]) -> Option<&RustcSpan> {
    spans.iter().find(|s| s.is_primary).or_else(|| spans.first())
}

fn span_to_our(s: &RustcSpan) -> Span {
    // rustc: line/column are 1-based code points — convert to 0-based.
    Span {
        start: Position {
            line: s.line_start.saturating_sub(1),
            column: s.column_start.saturating_sub(1),
        },
        end: Some(Position {
            line: s.line_end.saturating_sub(1),
            column: s.column_end.saturating_sub(1),
        }),
    }
}

fn normalize_level(level: &str) -> String {
    match level {
        "error" | "error: internal compiler error" => "error",
        "warning" => "warning",
        "note" => "note",
        "help" => "help",
        _ => "note",
    }
    .to_string()
}

fn convert(msg: &RustcMessage) -> Diagnostic {
    let span = pick_primary(&msg.spans).map(span_to_our);
    // Secondary spans with labels — become "note" details pointing to that location.
    let mut details: Vec<Diagnostic> = msg
        .spans
        .iter()
        .filter(|s| !s.is_primary && s.label.is_some())
        .map(|s| Diagnostic {
            level: "note".to_string(),
            span: Some(span_to_our(s)),
            message: s.label.clone().unwrap_or_default(),
            code: None,
            details: Vec::new(),
        })
        .collect();
    // Nested children (notes, helps) — recurse.
    details.extend(msg.children.iter().map(convert));
    Diagnostic {
        level: normalize_level(&msg.level),
        span,
        message: msg.message.clone(),
        code: msg.code.as_ref().map(|c| c.code.clone()),
        details,
    }
}

fn try_main() -> Result<i32, (i32, String)> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        return Err((
            1,
            "usage: judge-rust-compile <diagnostics-file> <rustc args...>".to_string(),
        ));
    }
    let diagnostics_path = Path::new(&args[1]).to_path_buf();
    let rustc_args: Vec<&str> = args[2..].iter().map(|s| s.as_str()).collect();

    let mut child = Command::new("rustc")
        .args(&rustc_args)
        .arg("--error-format=json")
        .arg("--json=diagnostic-rendered-ansi")
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| (1, format!("cannot run rustc: {}", e)))?;

    // Parse rustc JSON line-by-line, stream rendered to stderr, write NDJSON diagnostics.
    let mut diag_file = fs::File::create(&diagnostics_path)
        .map_err(|e| (1, format!("cannot create diagnostics file: {}", e)))?;
    if let Some(stderr_pipe) = child.stderr.take() {
        let reader = BufReader::new(stderr_pipe);
        let mut err = io::stderr();
        for line in reader.lines() {
            let line = line.map_err(|e| (1, format!("cannot read rustc stderr: {}", e)))?;
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<RustcMessage>(&line) {
                Ok(msg) => {
                    if let Some(rendered) = &msg.rendered {
                        err.write_all(rendered.as_bytes()).ok();
                    }
                    if let Ok(json) = serde_json::to_string(&convert(&msg)) {
                        let _ = writeln!(diag_file, "{}", json);
                    }
                }
                Err(_) => {
                    writeln!(err, "{}", line).ok();
                }
            }
        }
    }

    let status = child
        .wait()
        .map_err(|e| (1, format!("cannot wait rustc: {}", e)))?;

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
