#!/usr/bin/env python3
"""Local Ollama benchmark for LLM-oriented JapalityBean generation.

The benchmark compares compile/check success across JapalityBean, C, and Rust,
and can run a JSON-diagnostic repair loop for JapalityBean.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import tempfile
import time
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

TASKS = [
    {
        "name": "clamp_i32",
        "desc": "Implement clamp_i32: return low if x < low, high if x > high, otherwise x.",
        "jb_sig": "@func clamp_i32\n@intent: Clamp an i32 into inclusive bounds\n@in x::i32\n@in low::i32\n@in high::i32\n@out result::i32",
        "c_sig": "int clamp_i32(int x, int low, int high)",
        "rs_sig": "pub fn clamp_i32(x: i32, low: i32, high: i32) -> i32",
    },
    {
        "name": "find_max_even",
        "desc": "Implement find_max_even: given integers, return the maximum even number; return -1 if none exist.",
        "jb_sig": "@func find_max_even\n@intent: Find maximum even number or -1\n@in nums::Vector<i32>\n@out result::i32",
        "c_sig": "int find_max_even(const int *nums, long len)",
        "rs_sig": "pub fn find_max_even(nums: &[i32]) -> i32",
    },
    {
        "name": "sum_positive_even",
        "desc": "Implement sum_positive_even: sum only values that are both positive and even.",
        "jb_sig": "@func sum_positive_even\n@intent: Sum positive even numbers\n@in nums::Vector<i32>\n@out result::i32",
        "c_sig": "int sum_positive_even(const int *nums, long len)",
        "rs_sig": "pub fn sum_positive_even(nums: &[i32]) -> i32",
    },
]


def read_guide() -> str:
    guide = ROOT / "docs" / "llm-authoring-guide.md"
    return guide.read_text()


def ollama_generate(model: str, prompt: str, temperature: float, num_predict: int) -> str:
    payload = {
        "model": model,
        "prompt": prompt,
        "stream": False,
        "options": {
            "temperature": temperature,
            "num_predict": num_predict,
        },
    }
    data = json.dumps(payload).encode()
    request = urllib.request.Request(
        "http://127.0.0.1:11434/api/generate",
        data=data,
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=300) as response:
        return json.loads(response.read().decode())["response"]


def extract_code(text: str) -> str:
    match = re.search(r"```(?:[A-Za-z0-9_+-]+)?\s*(.*?)```", text, re.S)
    return (match.group(1) if match else text).strip()


def prompt_for(lang: str, task: dict[str, str], guide: str) -> str:
    if lang == "japalitybean":
        return f"""You are writing JapalityBean.
Use the guide below exactly.

{guide}

Task: {task["desc"]}
Required function header:
{task["jb_sig"]}

Return ONLY a complete compilable JapalityBean source file."""
    if lang == "c":
        return f"""Write ISO C code. Return ONLY compilable C source code, no Markdown.
Task: {task["desc"]}
Required function signature: {task["c_sig"]}
Return a complete source file with this function. Do not include main."""
    if lang == "rust":
        return f"""Write Rust code. Return ONLY compilable Rust source code, no Markdown.
Task: {task["desc"]}
Required function signature: {task["rs_sig"]}
Return a complete library source file with this function. Do not include main."""
    raise ValueError(f"unknown lang: {lang}")


def validate(lang: str, source: str, workdir: Path, name: str) -> dict:
    suffix = {"japalitybean": "jb", "c": "c", "rust": "rs"}[lang]
    path = workdir / f"{name}.{suffix}"
    path.write_text(source)
    if lang == "japalitybean":
        cmd = ["cargo", "run", "--quiet", "--", "check", str(path), "--json-batch"]
        cwd = ROOT
    elif lang == "c":
        cmd = ["gcc", "-std=c11", "-Wall", "-Wextra", "-fsyntax-only", str(path)]
        cwd = workdir
    else:
        cmd = ["rustc", "--crate-type", "lib", str(path), "-o", str(workdir / f"{name}.rlib")]
        cwd = workdir
    started = time.time()
    proc = subprocess.run(
        cmd,
        cwd=cwd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=90,
    )
    return {
        "ok": proc.returncode == 0,
        "returncode": proc.returncode,
        "stdout": proc.stdout,
        "stderr": proc.stderr,
        "validate_sec": round(time.time() - started, 3),
        "bytes": len(source.encode()),
        "lines": len([line for line in source.splitlines() if line.strip()]),
        "path": str(path),
    }


def repair_prompt(guide: str, source: str, diagnostics: str) -> str:
    return f"""Fix this JapalityBean source so jbc check passes.
Return ONLY complete corrected source.
Do not include Markdown, explanations, bullets, JSON, diffs, braces, or square brackets.
If diagnostics mention redeclared `result`, remove `let result::...` and use `set result = ...`.

{guide}

Previous source:
{source}

Compiler JSON diagnostics:
{diagnostics}
"""


def summarize(rows: list[dict]) -> dict:
    by_lang = {}
    for lang in sorted({row["lang"] for row in rows}):
        selected = [row for row in rows if row["lang"] == lang]
        by_lang[lang] = {
            "n": len(selected),
            "success": sum(row["ok"] for row in selected),
            "success_rate": round(sum(row["ok"] for row in selected) / len(selected), 3),
            "initial_success": sum(row["attempts"][0]["ok"] for row in selected),
            "avg_attempts": round(sum(len(row["attempts"]) for row in selected) / len(selected), 2),
            "avg_lines": round(sum(row["attempts"][-1]["lines"] for row in selected) / len(selected), 1),
            "avg_bytes": round(sum(row["attempts"][-1]["bytes"] for row in selected) / len(selected), 1),
        }
    return by_lang


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", default="llama3.1:8b")
    parser.add_argument("--langs", default="japalitybean,c,rust")
    parser.add_argument("--reps", type=int, default=2)
    parser.add_argument("--max-repairs", type=int, default=2)
    parser.add_argument("--temperature", type=float, default=0.2)
    parser.add_argument("--num-predict", type=int, default=900)
    parser.add_argument("--tasks", default=",".join(task["name"] for task in TASKS))
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    guide = read_guide()
    langs = [lang.strip() for lang in args.langs.split(",") if lang.strip()]
    task_names = {task.strip() for task in args.tasks.split(",") if task.strip()}
    tasks = [task for task in TASKS if task["name"] in task_names]
    rows = []

    with tempfile.TemporaryDirectory(prefix="jb-ollama-bench-") as temp:
        workdir = Path(temp)
        for task in tasks:
            for lang in langs:
                for rep in range(args.reps):
                    attempts = []
                    source = ""
                    diagnostics = ""
                    for attempt in range(args.max_repairs + 1):
                        if attempt > 0 and lang != "japalitybean":
                            break
                        prompt = (
                            prompt_for(lang, task, guide)
                            if attempt == 0
                            else repair_prompt(guide, source, diagnostics)
                        )
                        started = time.time()
                        raw = ollama_generate(args.model, prompt, args.temperature, args.num_predict)
                        source = extract_code(raw)
                        validation = validate(lang, source, workdir, f"{task['name']}_{lang}_{rep}_{attempt}")
                        diagnostics = validation["stdout"] or validation["stderr"]
                        attempt_row = {
                            "attempt": attempt,
                            "ok": validation["ok"],
                            "gen_sec": round(time.time() - started, 2),
                            "lines": validation["lines"],
                            "bytes": validation["bytes"],
                            "diagnostics": diagnostics[:1200],
                            "path": validation["path"],
                        }
                        attempts.append(attempt_row)
                        if validation["ok"]:
                            break
                    row = {
                        "task": task["name"],
                        "lang": lang,
                        "rep": rep,
                        "ok": attempts[-1]["ok"],
                        "attempts": attempts,
                    }
                    rows.append(row)
                    print(json.dumps(row, ensure_ascii=False), flush=True)

    result = {
        "model": args.model,
        "reps": args.reps,
        "max_repairs": args.max_repairs,
        "summary": summarize(rows),
        "rows": rows,
    }
    print("SUMMARY_JSON=" + json.dumps(result["summary"], ensure_ascii=False))
    if args.output:
        args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
