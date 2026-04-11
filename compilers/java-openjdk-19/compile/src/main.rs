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

/// Extract the public class name from Java source.
/// Falls back to "Main" if no public class found.
fn get_class_name(path: &Path) -> Result<String, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
    let re = Regex::new(r"\s*public\s+class\s+([a-zA-Z0-9_]+)").unwrap();
    match re.captures(&content) {
        Some(caps) => Ok(caps[1].to_string()),
        None => Ok("Main".to_string()),
    }
}

fn write_manifest(path: &Path, class_name: &str) -> Result<(), String> {
    let mut file = fs::File::create(path)
        .map_err(|e| format!("cannot create manifest: {}", e))?;
    write!(file, "Manifest-Version: 1.0\nMain-Class: {}\n", class_name)
        .map_err(|e| format!("cannot write manifest: {}", e))?;
    file.sync_all()
        .map_err(|e| format!("cannot sync manifest: {}", e))?;
    Ok(())
}

/// Parse javac stderr output into diagnostics.
///
/// javac format:
///   filename.java:LINE: error: MESSAGE
///   SOURCE_LINE
///               ^
///   filename.java:LINE: warning: MESSAGE
///   SOURCE_LINE
///                 ^
fn parse_javac_diagnostics(stderr: &str, source_name: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    // Pattern: filename.java:LINE: error: MESSAGE  or  filename.java:LINE: warning: MESSAGE
    let pattern = format!(
        r"{}:(\d+): (error|warning): (.+)",
        regex::escape(source_name)
    );
    let re = Regex::new(&pattern).unwrap();
    // Caret pattern — a line with only spaces and a single ^
    let caret_re = Regex::new(r"^(\s*)\^$").unwrap();

    let lines: Vec<&str> = stderr.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if let Some(caps) = re.captures(lines[i]) {
            let line_num: usize = caps[1].parse().unwrap_or(1);
            let level = caps[2].to_string();
            let message = caps[3].to_string();

            // Look ahead for source line + caret to determine column.
            let mut column = None;
            // Next line is the source line, line after is the caret.
            if i + 2 < lines.len() {
                if let Some(caret_caps) = caret_re.captures(lines[i + 2]) {
                    // Column is the number of unicode code points before ^.
                    column = Some(caret_caps[1].chars().count());
                    i += 2; // skip source line and caret line
                }
            }

            let span = Some(Span {
                start: Position {
                    line: line_num.saturating_sub(1), // 0-based
                    column: column.unwrap_or(0),
                },
                end: None,
            });

            diagnostics.push(Diagnostic {
                level,
                span,
                message,
                code: None,
                details: Vec::new(),
            });
        }
        i += 1;
    }
    diagnostics
}

fn run_cmd(name: &str, args: &[&str]) -> Result<(), (i32, String)> {
    let status = Command::new(name)
        .args(args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| (1, format!("cannot run {}: {}", name, e)))?;
    if status.success() {
        Ok(())
    } else {
        let code = status.code().unwrap_or(1);
        Err((code, String::new()))
    }
}

fn try_main() -> Result<(), (i32, String)> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err((1, "usage: judge-java-compile <input> <output> [diagnostics]".to_string()));
    }
    let input_file = Path::new(&args[1]).to_path_buf();
    let output_file = Path::new(&args[2]).to_path_buf();
    let diagnostics_file = args.get(3).map(|s| Path::new(s).to_path_buf());

    let class_name = get_class_name(&input_file)
        .map_err(|e| (1, e))?;

    let dir = input_file.parent().unwrap_or(Path::new("."));
    let java_file = dir.join(format!("{}.java", class_name));
    let manifest_file = dir.join("manifest.txt");

    // Rename to match class name if needed.
    let renamed = input_file != java_file;
    if renamed {
        fs::rename(&input_file, &java_file)
            .map_err(|e| (1, format!("cannot rename {:?} to {:?}: {}", input_file, java_file, e)))?;
    }

    let result = (|| -> Result<(), (i32, String)> {
        write_manifest(&manifest_file, &class_name)
            .map_err(|e| (1, e))?;

        // Run javac — stream stderr in real-time while capturing for diagnostics.
        let mut javac = Command::new("javac")
            .arg(&java_file)
            .stdout(Stdio::inherit())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| (1, format!("cannot run javac: {}", e)))?;

        // Tee stderr: forward each chunk and collect for parsing.
        let mut captured_stderr = Vec::new();
        if let Some(stderr_pipe) = javac.stderr.take() {
            let mut reader = BufReader::new(stderr_pipe);
            let mut err = io::stderr();
            let mut buf = Vec::new();
            loop {
                let n = reader.read_until(b'\n', &mut buf)
                    .map_err(|e| (1, format!("cannot read javac stderr: {}", e)))?;
                if n == 0 {
                    break;
                }
                err.write_all(&buf).ok();
                captured_stderr.extend_from_slice(&buf);
                buf.clear();
            }
        }
        let captured_stderr = String::from_utf8_lossy(&captured_stderr).to_string();

        let javac_status = javac.wait()
            .map_err(|e| (1, format!("cannot wait javac: {}", e)))?;

        // Parse diagnostics if requested.
        if let Some(ref diag_path) = diagnostics_file {
            let source_name = java_file
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            let diagnostics = parse_javac_diagnostics(&captured_stderr, &source_name);
            if let Ok(json) = serde_json::to_string(&diagnostics) {
                fs::write(diag_path, json).ok();
            }
        }

        if !javac_status.success() {
            let code = javac_status.code().unwrap_or(1);
            return Err((code, String::new()));
        }

        // Collect .class files and build jar.
        let mut jar_args: Vec<&str> = vec!["cfm"];
        let output_str = output_file.to_str().unwrap();
        let manifest_str = manifest_file.to_str().unwrap();
        jar_args.push(output_str);
        jar_args.push(manifest_str);

        let class_files: Vec<String> = fs::read_dir(dir)
            .map_err(|e| (1, format!("cannot read dir: {}", e)))?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".class") { Some(name) } else { None }
            })
            .collect();

        let mut jar_args_owned: Vec<String> = vec![
            "cfm".to_string(),
            output_str.to_string(),
            manifest_str.to_string(),
        ];
        jar_args_owned.extend(class_files);
        let jar_args_ref: Vec<&str> = jar_args_owned.iter().map(|s| s.as_str()).collect();

        run_cmd("jar", &jar_args_ref)?;
        Ok(())
    })();

    // Restore original file name.
    if renamed {
        fs::rename(&java_file, &input_file).ok();
    }

    result
}

fn main() -> ExitCode {
    match try_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, msg)) => {
            if !msg.is_empty() {
                eprintln!("{}", msg);
            }
            ExitCode::from(code as u8)
        }
    }
}
