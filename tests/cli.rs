use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn jbc() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jbc"))
}

fn run(args: &[&str]) -> Output {
    Command::new(jbc())
        .args(args)
        .output()
        .expect("jbc command should run")
}

fn stdout_json(output: &Output) -> Value {
    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout should be UTF-8");
    serde_json::from_str(stdout.trim()).expect("stdout should be valid JSON")
}

fn temp_path(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("jbc-test-{name}-{nonce}"))
}

fn write_source(path: &Path, source: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directory should be created");
    }
    fs::write(path, source).expect("source should be written");
}

#[test]
fn check_valid_example_emits_empty_json_batch() {
    let output = run(&["check", "examples/find_max_even.jb", "--json-batch"]);

    assert!(output.status.success());
    assert_eq!(stdout_json(&output), serde_json::json!([]));
}

#[test]
fn check_type_error_emits_machine_readable_diagnostic() {
    let output = run(&["check", "examples/type_error.jb", "--json-batch"]);

    assert_eq!(output.status.code(), Some(1));
    let json = stdout_json(&output);
    let diagnostics = json.as_array().expect("diagnostics should be an array");
    assert!(diagnostics.iter().any(|diag| {
        diag["code"] == "E_TYPE002"
            && diag["phase"] == "type"
            && diag["expected"] == "i32"
            && diag["actual"] == "string"
    }));
}

#[test]
fn build_ast_json_is_valid_json() {
    let output = run(&["build", "examples/find_max_even.jb", "--emit=ast-json"]);

    assert!(output.status.success());
    let json = stdout_json(&output);
    assert_eq!(json["functions"][0]["name"], "find_max_even");
    assert_eq!(json["functions"][0]["output"]["type"], "i32");
}

#[test]
fn build_ir_emits_llvm_text_for_loop_example() {
    let output = run(&["build", "examples/find_max_even.jb", "--emit=ir"]);

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    assert!(stdout.contains("define internal i32 @jb_find_max_even"));
    assert!(stdout.contains("br label %loop.header"));
    assert!(stdout.contains("icmp"));
    assert!(!stdout.contains("pseudo LLVM IR"));
}

#[test]
fn build_exe_compiles_and_runs_on_linux() {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return;
    }

    let exe = temp_path("exe").join("main");
    let output = run(&[
        "build",
        "examples/linux_main.jb",
        "--emit=exe",
        "-o",
        exe.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let bytes = fs::read(&exe).expect("native executable should be readable");
    assert_eq!(&bytes[..4], b"\x7fELF");

    let run_output = Command::new(&exe)
        .output()
        .expect("compiled executable should run");
    assert_eq!(run_output.status.code(), Some(7));
}

#[test]
fn native_exe_supports_calls_builtins_conditions_and_loops() {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return;
    }

    let calls_exe = temp_path("native-calls").join("main");
    let calls = run(&[
        "build",
        "examples/native_calls.jb",
        "--emit=exe",
        "-o",
        calls_exe.to_str().unwrap(),
    ]);
    assert!(calls.status.success(), "stderr: {}", String::from_utf8_lossy(&calls.stderr));
    assert_eq!(
        Command::new(&calls_exe)
            .output()
            .expect("native calls executable should run")
            .status
            .code(),
        Some(5)
    );

    let condition_exe = temp_path("native-condition").join("main");
    let condition = run(&[
        "build",
        "examples/native_condition.jb",
        "--emit=exe",
        "-o",
        condition_exe.to_str().unwrap(),
    ]);
    assert!(
        condition.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&condition.stderr)
    );
    assert_eq!(
        Command::new(&condition_exe)
            .output()
            .expect("native condition executable should run")
            .status
            .code(),
        Some(42)
    );

    let loop_exe = temp_path("native-loop").join("main");
    let loop_output = run(&[
        "build",
        "examples/find_max_even.jb",
        "--emit=exe",
        "--entry",
        "find_max_even",
        "-o",
        loop_exe.to_str().unwrap(),
    ]);
    assert!(
        loop_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&loop_output.stderr)
    );
    assert_eq!(
        Command::new(&loop_exe)
            .output()
            .expect("native loop executable should run")
            .status
            .code(),
        Some(255)
    );
}

#[test]
fn native_runtime_reads_argv_and_writes_debug_i32() {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return;
    }

    let exe = temp_path("native-runtime").join("main");
    let output = run(&[
        "build",
        "examples/native_runtime.jb",
        "--emit=exe",
        "-o",
        exe.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let run_output = Command::new(&exe)
        .arg("12")
        .output()
        .expect("native runtime executable should run");
    assert_eq!(run_output.status.code(), Some(19));
    assert_eq!(String::from_utf8(run_output.stdout).unwrap(), "19\n");
}

#[test]
fn native_runtime_writes_static_string_literals() {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return;
    }

    let exe = temp_path("native-string").join("main");
    let output = run(&[
        "build",
        "examples/native_string.jb",
        "--emit=exe",
        "-o",
        exe.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let run_output = Command::new(&exe)
        .arg("23")
        .output()
        .expect("native string executable should run");
    assert_eq!(run_output.status.code(), Some(23));
    assert_eq!(
        String::from_utf8(run_output.stdout).unwrap(),
        "hello from JapalityBean\n"
    );
}

#[test]
fn native_runtime_constructs_and_loops_over_vector_i32() {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return;
    }

    let exe = temp_path("native-vector").join("main");
    let output = run(&[
        "build",
        "examples/native_vector.jb",
        "--emit=exe",
        "-o",
        exe.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

    let run_output = Command::new(&exe)
        .arg("4")
        .output()
        .expect("native vector executable should run");
    assert_eq!(run_output.status.code(), Some(10));
    assert_eq!(String::from_utf8(run_output.stdout).unwrap(), "10\n");
}

#[test]
fn formatter_is_idempotent() {
    let path = temp_path("fmt").join("main.jb");
    write_source(
        &path,
        "@func main_value\n@intent: Return the seed\n@in seed::i32\n@out result::i32\n---\nlet value::i32 = seed\nset result = value\nreturn result\n@end func main_value\n",
    );

    let first = run(&["fmt", path.to_str().unwrap()]);
    assert!(first.status.success());
    let once = fs::read_to_string(&path).expect("formatted source should exist");

    let second = run(&["fmt", path.to_str().unwrap()]);
    assert!(second.status.success());
    let twice = fs::read_to_string(&path).expect("formatted source should exist");

    assert_eq!(once, twice);
}

#[test]
fn project_mode_ignores_hidden_appledouble_files() {
    let root = temp_path("project");
    write_source(
        &root.join("jb.toml"),
        "name = \"sample_project\"\nversion = \"0.1.0\"\nentrypoint = \"src/main.jb\"\n",
    );
    write_source(
        &root.join("src/main.jb"),
        "@func main_value\n@intent: Return the seed\n@in seed::i32\n@out result::i32\n---\nset result = seed\nreturn result\n@end func main_value\n",
    );
    fs::write(root.join("src/._main.jb"), [0xff, 0xfe, 0xfd]).expect("hidden file should be written");

    let output = run(&["check", root.to_str().unwrap(), "--json-batch"]);

    assert!(output.status.success());
    assert_eq!(stdout_json(&output), serde_json::json!([]));
}

#[test]
fn cast_option_and_result_constructors_type_check() {
    let path = temp_path("types").join("main.jb");
    write_source(
        &path,
        "@func cast_demo\n@intent: Cast an integer explicitly\n@in seed::i32\n@out result::i64\n---\nlet widened::i64 = seed as::<i64>\nset result = widened\nreturn result\n@end func cast_demo\n\n@func option_demo\n@intent: Construct an optional integer\n@in seed::i32\n@out result::Option<i32>\n---\nset result = Some(seed)\nreturn result\n@end func option_demo\n\n@func none_demo\n@intent: Construct an empty optional integer\n@in seed::i32\n@out result::Option<i32>\n---\nlet marker::i32 = seed\nlet empty::Option<i32> = None\nset result = empty\nreturn result\n@end func none_demo\n\n@func result_demo\n@intent: Construct an ok result\n@in seed::i32\n@out result::Result<i32,string>\n---\nset result = Ok(seed)\nreturn result\n@end func result_demo\n",
    );

    let output = run(&["check", path.to_str().unwrap(), "--json-batch"]);

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(stdout_json(&output), serde_json::json!([]));
}

#[test]
fn implicit_numeric_widening_requires_cast() {
    let path = temp_path("widening").join("main.jb");
    write_source(
        &path,
        "@func widening_demo\n@intent: Reject implicit numeric widening\n@in seed::i32\n@out result::i64\n---\nlet widened::i64 = seed\nset result = widened\nreturn result\n@end func widening_demo\n",
    );

    let output = run(&["check", path.to_str().unwrap(), "--json-batch"]);

    assert_eq!(output.status.code(), Some(1));
    let diagnostics = stdout_json(&output);
    assert!(diagnostics
        .as_array()
        .expect("diagnostics should be an array")
        .iter()
        .any(|diag| diag["code"] == "E_TYPE003"));
}
