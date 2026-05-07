mod ast;
mod diag;
mod ir;
mod lexer;
mod native;
mod parser;
mod pretty;
mod typeck;

use crate::diag::{emit_json_batch, emit_json_lines, Diagnostic};
use crate::parser::parse_program_from_tokens;
use crate::pretty::{pretty_program, program_json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Check,
    Build,
    Fmt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmitKind {
    Ir,
    AstJson,
    Obj,
    Exe,
}

#[derive(Debug)]
struct Cli {
    command: Command,
    path: PathBuf,
    json: bool,
    json_batch: bool,
    emit: EmitKind,
    output: Option<PathBuf>,
    entry: String,
}

fn main() -> ExitCode {
    match parse_cli(env::args().skip(1).collect()) {
        Ok(cli) => run(cli),
        Err(message) => {
            eprintln!("{message}");
            eprintln!("{}", usage());
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> ExitCode {
    match cli.command {
        Command::Check => run_check(&cli),
        Command::Build => run_build(&cli),
        Command::Fmt => run_fmt(&cli),
    }
}

fn run_check(cli: &Cli) -> ExitCode {
    let (_, all_diags) = match check_path(&cli.path) {
        Ok(result) => result,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    emit_diags(cli, &all_diags);
    if Diagnostic::has_errors(&all_diags) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn run_build(cli: &Cli) -> ExitCode {
    let files = collect_jb_files(&cli.path);
    let (program, all_diags) = match check_files(&files) {
        Ok(result) => result,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };

    emit_diags(cli, &all_diags);
    if Diagnostic::has_errors(&all_diags) {
        return ExitCode::from(1);
    }

    match cli.emit {
        EmitKind::Ir => {
            print!("{}", ir::lower_program_to_ir(&program));
            ExitCode::SUCCESS
        }
        EmitKind::AstJson => {
            print!("{}", program_json(&program));
            ExitCode::SUCCESS
        }
        EmitKind::Obj => {
            let artifact = ir::lower_program_to_ir(&program);
            let output = cli
                .output
                .clone()
                .unwrap_or_else(|| default_output_path(&cli.path, cli.emit));
            match compile_llvm_ir(&artifact, &output, cli.emit) {
                Ok(()) => ExitCode::SUCCESS,
                Err(message) => {
                    eprintln!("{message}");
                    ExitCode::from(2)
                }
            }
        }
        EmitKind::Exe => {
            let output = cli
                .output
                .clone()
                .unwrap_or_else(|| default_output_path(&cli.path, cli.emit));
            match write_native_executable(&program, &cli.entry, &output) {
                Ok(()) => ExitCode::SUCCESS,
                Err(message) => {
                    eprintln!("{message}");
                    ExitCode::from(2)
                }
            }
        }
    }
}

fn run_fmt(cli: &Cli) -> ExitCode {
    let files = collect_jb_files(&cli.path);
    let mut all_diags = Vec::new();
    for file in files {
        let source = match fs::read_to_string(&file) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("failed to read {}: {err}", file.display());
                return ExitCode::from(2);
            }
        };
        let (program, mut diags) = parse_source(&source);
        let has_errors = Diagnostic::has_errors(&diags);
        all_diags.append(&mut diags);
        if !has_errors {
            if let Err(err) = fs::write(&file, pretty_program(&program)) {
                eprintln!("failed to write {}: {err}", file.display());
                return ExitCode::from(2);
            }
        }
    }
    emit_diags(cli, &all_diags);
    if Diagnostic::has_errors(&all_diags) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn check_path(path: &Path) -> Result<(ast::Program, Vec<Diagnostic>), String> {
    let files = collect_jb_files(path);
    check_files(&files)
}

fn check_files(files: &[PathBuf]) -> Result<(ast::Program, Vec<Diagnostic>), String> {
    let mut combined = ast::Program {
        functions: Vec::new(),
    };
    let mut diags = Vec::new();

    for file in files {
        let source = fs::read_to_string(file)
            .map_err(|err| format!("failed to read {}: {err}", file.display()))?;
        let (mut program, mut file_diags) = parse_source(&source);
        combined.functions.append(&mut program.functions);
        diags.append(&mut file_diags);
    }

    if !Diagnostic::has_errors(&diags) {
        diags.extend(typeck::check_program(&combined));
    }
    Ok((combined, diags))
}

fn parse_source(source: &str) -> (ast::Program, Vec<Diagnostic>) {
    let (tokens, mut diags) = lexer::lex_source(source);
    if Diagnostic::has_errors(&diags) {
        return (
            ast::Program {
                functions: Vec::new(),
            },
            diags,
        );
    }
    let (program, mut parse_diags) = parse_program_from_tokens(source, &tokens);
    diags.append(&mut parse_diags);
    (program, diags)
}

fn emit_diags(cli: &Cli, diags: &[Diagnostic]) {
    if cli.json_batch {
        emit_json_batch(diags);
    } else if cli.json {
        emit_json_lines(diags);
    } else if diags.is_empty() {
        eprintln!("ok");
    } else {
        emit_json_lines(diags);
    }
}

fn collect_jb_files(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        return vec![path.to_path_buf()];
    }
    let root = if path.join("jb.toml").exists() {
        path.join("src")
    } else {
        path.to_path_buf()
    };
    let mut files = Vec::new();
    collect_recursive(&root, &mut files);
    files.sort();
    files
}

fn collect_recursive(path: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.starts_with('.'))
            .unwrap_or(false)
        {
            continue;
        }
        if path.is_dir() {
            collect_recursive(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jb") {
            out.push(path);
        }
    }
}

fn compile_llvm_ir(ir_text: &str, output: &Path, emit: EmitKind) -> Result<(), String> {
    let temp_ir = temp_ir_path();
    fs::write(&temp_ir, ir_text)
        .map_err(|err| format!("failed to write temporary LLVM IR {}: {err}", temp_ir.display()))?;

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create output directory {}: {err}", parent.display()))?;
        }
    }

    let compiler = find_c_compiler().ok_or_else(|| {
        "failed to find clang, cc, or gcc on PATH; install clang to build executables".to_string()
    })?;
    let mut cmd = ProcessCommand::new(&compiler);
    cmd.arg("-x").arg("ir").arg(&temp_ir).arg("-o").arg(output);
    if emit == EmitKind::Obj {
        cmd.arg("-c");
    }

    let result = cmd
        .output()
        .map_err(|err| format!("failed to run {}: {err}", compiler.display()))?;
    let _ = fs::remove_file(&temp_ir);
    if result.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&result.stderr);
    Err(format!(
        "LLVM backend command failed ({}): {}",
        compiler.display(),
        stderr.trim()
    ))
}

fn write_native_executable(
    program: &ast::Program,
    entry: &str,
    output: &Path,
) -> Result<(), String> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Err("native executable backend currently targets Linux x86_64 only".to_string());
    }
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create output directory {}: {err}", parent.display()))?;
        }
    }
    let bytes = native::emit_linux_x86_64_executable(program, entry)?;
    fs::write(output, bytes)
        .map_err(|err| format!("failed to write native executable {}: {err}", output.display()))?;
    make_executable(output)?;
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    let mut permissions = fs::metadata(path)
        .map_err(|err| format!("failed to stat {}: {err}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .map_err(|err| format!("failed to chmod {}: {err}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn find_c_compiler() -> Option<PathBuf> {
    env::var_os("JBC_CC")
        .map(PathBuf::from)
        .or_else(|| find_on_path("clang"))
        .or_else(|| find_on_path("cc"))
        .or_else(|| find_on_path("gcc"))
}

fn find_on_path(binary: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    env::split_paths(&paths)
        .map(|path| path.join(binary))
        .find(|candidate| candidate.is_file())
}

fn temp_ir_path() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    env::temp_dir().join(format!("jbc-{nonce}-{}.ll", std::process::id()))
}

fn default_output_path(input: &Path, emit: EmitKind) -> PathBuf {
    match emit {
        EmitKind::Obj => input.with_extension("o"),
        EmitKind::Exe => PathBuf::from("a.out"),
        EmitKind::Ir | EmitKind::AstJson => unreachable!(),
    }
}

fn parse_cli(args: Vec<String>) -> Result<Cli, String> {
    if args.is_empty() {
        return Err("missing subcommand".to_string());
    }

    let mut json = false;
    let mut json_batch = false;
    let mut emit = EmitKind::Ir;
    let mut command = None;
    let mut path = None;
    let mut output = None;
    let mut entry = "main".to_string();

    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--json-batch" => json_batch = true,
            "check" => command = Some(Command::Check),
            "build" => command = Some(Command::Build),
            "fmt" => command = Some(Command::Fmt),
            _ if arg.starts_with("--emit=") => {
                emit = match arg.trim_start_matches("--emit=") {
                    "ir" => EmitKind::Ir,
                    "ast-json" => EmitKind::AstJson,
                    "obj" => EmitKind::Obj,
                    "exe" => EmitKind::Exe,
                    other => return Err(format!("unknown emit kind: {other}")),
                };
            }
            _ if arg == "--emit" => {
                let Some(value) = iter.next() else {
                    return Err("--emit requires a value".to_string());
                };
                emit = match value.as_str() {
                    "ir" => EmitKind::Ir,
                    "ast-json" => EmitKind::AstJson,
                    "obj" => EmitKind::Obj,
                    "exe" => EmitKind::Exe,
                    other => return Err(format!("unknown emit kind: {other}")),
                };
            }
            "-o" | "--output" => {
                let Some(value) = iter.next() else {
                    return Err(format!("{arg} requires a path"));
                };
                output = Some(PathBuf::from(value));
            }
            _ if arg.starts_with("--output=") => {
                output = Some(PathBuf::from(arg.trim_start_matches("--output=")));
            }
            "--entry" => {
                let Some(value) = iter.next() else {
                    return Err("--entry requires a function name".to_string());
                };
                entry = value;
            }
            _ if arg.starts_with("--entry=") => {
                entry = arg.trim_start_matches("--entry=").to_string();
            }
            _ if arg.starts_with('-') => return Err(format!("unknown flag: {arg}")),
            _ => path = Some(PathBuf::from(arg)),
        }
    }

    Ok(Cli {
        command: command.ok_or_else(|| "missing subcommand".to_string())?,
        path: path.ok_or_else(|| "missing path".to_string())?,
        json,
        json_batch,
        emit,
        output,
        entry,
    })
}

fn usage() -> &'static str {
    "usage: jbc [--json|--json-batch] <check|build|fmt> <path> [--emit=ir|ast-json|obj|exe] [-o <path>] [--entry <func>]"
}
