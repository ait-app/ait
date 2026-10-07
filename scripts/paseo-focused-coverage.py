#!/usr/bin/env python3
"""Run only the affected Paseo compatibility tests and publish reviewable LCOV line evidence."""

import argparse
import hashlib
import json
import re
import subprocess
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
# Keep changed-line evidence reproducible after the audit itself has been committed.
BASE_REVISION = "b797e0d2f83ae57f62892c288a5d81776f8afa6a"
REPORT = ROOT / "docs/reports/daemon/paseo-server-coverage-2026-09-29"
TEMP = ROOT / "target/paseo-focused-coverage"
SCOPES = [
    ("file", ["--lib"], ["config::", "registry::", "single::", "watch::", "creation::", "storage::"]),
    ("provider", ["--lib"], [
        "service::agent_execution::", "service::agent_manager::", "service::agent_runtime::",
        "service::provider_catalog::", "service::workspace_attention::", "rpc::timeline::",
        "rpc::fork_context::", "rpc::agent_execution::", "rpc::agent_runtime::", "connection::",
        "local::codex::", "local::claude::", "storage::timeline::", "ports::environment::",
        "ports::agent_session::", "protocol::tests::",
    ]),
    ("model", ["--lib"], [
        "pagination::", "directory_sync::", "polling::", "runtime::", "events::",
        "workspace::", "storage::", "session::", "creation::", "summary::",
    ]),
    ("metadata", ["--lib"], [
        "service::directory::", "workspace_automation::", "rpc::directory::",
    ]),
    ("filesystem", ["--lib"], ["worktrees::", "worktree_checkout::"]),
    ("terminal", ["--lib"], ["service::", "activity::"]),
    ("api", ["--lib"], [
        "terminal_activity::", "listener::", "capabilities::", "auth::", "browser_auth::",
        "tests::session::", "tests::paseo::",
    ]),
    ("daemon", ["--bin", "daemon"], ["host::tests::workspace_attention::"]),
    ("daemon", ["--test", "process"], [
        "agent_execution::", "agent_controls::", "agent_history::", "terminal::", "worktrees::",
        "workspace_automation::", "directory::", "native_sessions::", "schedule::", "session::",
    ]),
    ("protocol", ["--lib"], ["methods::"]),
]
IGNORE = r"/(tests|test_support)(/|\.rs$)"


def command(crate, target, filters):
    return ["cargo", "llvm-cov", "test", "--locked", "--offline", "-p", crate,
            *target, "--no-report", "--", *filters]


def run():
    TEMP.mkdir(parents=True, exist_ok=True)
    subprocess.run(["cargo", "llvm-cov", "clean", "--workspace"], cwd=ROOT, check=True)
    for crate, target, filters in SCOPES:
        with (TEMP / f"{crate}.log").open("w") as output:
            result = subprocess.run(command(crate, target, filters), cwd=ROOT,
                                    stdout=output, stderr=subprocess.STDOUT)
        if result.returncode:
            raise SystemExit(f"{crate} failed; inspect {TEMP / (crate + '.log')}")
        print(f"{crate}: focused tests passed", flush=True)
    with (TEMP / "export.log").open("w") as output:
        for arguments in [
            ["--lcov", "--output-path", str(TEMP / "coverage.lcov")],
            ["--html"],
        ]:
            subprocess.run(["cargo", "llvm-cov", "report", *arguments,
                            "--ignore-filename-regex", IGNORE], cwd=ROOT, check=True,
                           stdout=output, stderr=subprocess.STDOUT)


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True)


def parse_lcov(path):
    files = {}
    current = None
    for line in path.read_text().splitlines():
        if line.startswith("SF:"):
            current = str(Path(line[3:]).relative_to(ROOT))
            files.setdefault(current, {})
        elif line.startswith("DA:"):
            number, count, *_ = line[3:].split(",")
            files[current][int(number)] = int(count)
    return files


def changed_lines():
    changed = {}
    current = None
    for line in git("diff", "--no-ext-diff", "--unified=0", BASE_REVISION, "--", "bins", "crates").splitlines():
        if line.startswith("+++ b/"):
            current = line[6:]
        elif line.startswith("@@") and current:
            match = re.search(r"\+(\d+)(?:,(\d+))? @@", line)
            if match:
                start, count = int(match[1]), int(match[2] or 1)
                changed.setdefault(current, set()).update(range(start, start + count))
    for path in git("ls-files", "--others", "--exclude-standard", "--", "bins", "crates").splitlines():
        if path.endswith(".rs"):
            changed[path] = set(range(1, len((ROOT / path).read_text().splitlines()) + 1))
    return changed


def total(rows):
    rows = list(rows)
    covered = sum(row["covered"] for row in rows)
    count = sum(row["total"] for row in rows)
    return {"covered": covered, "total": count,
            "percent": round(100 * covered / count, 2) if count else None}


def line_counts(lines):
    return {"covered": sum(count > 0 for count in lines.values()), "total": len(lines)}


def report():
    raw = TEMP / "coverage.lcov"
    lines = parse_lcov(raw)
    changed = changed_lines()
    selected = {crate for crate, _, _ in SCOPES}
    files = []
    for path, execution in sorted(lines.items()):
        parts = Path(path).parts
        crate = "daemon" if parts[:2] == ("bins", "daemon") else parts[1]
        if crate not in selected or re.search(IGNORE, "/" + path):
            continue
        changed_execution = {line: count for line, count in execution.items()
                             if line in changed.get(path, set())}
        files.append({
            "path": path, "crate": crate,
            "source_sha256": hashlib.sha256((ROOT / path).read_bytes()).hexdigest(),
            "changed_file": path in changed,
            **line_counts(execution), "changed_lines": line_counts(changed_execution),
            "uncovered_lines": [line for line, count in execution.items() if count == 0],
            "changed_line_hits": changed_execution,
        })
    results = []
    for crate, target, filters in SCOPES:
        log = (TEMP / f"{crate}.log").read_text()
        match = re.search(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;.*? (\d+) filtered out", log)
        if not match:
            raise SystemExit(f"No successful focused result in {crate}.log")
        results.append({"crate": crate, "command": command(crate, target, filters),
                        "passed": int(match[1]), "failed": int(match[2]),
                        "ignored": int(match[3]), "filtered_out": int(match[4]),
                        "log_sha256": hashlib.sha256(log.encode()).hexdigest()})
    changed_files = [file for file in files if file["changed_file"]]
    artifact = {
        "measured_at_utc": datetime.now(timezone.utc).isoformat(),
        "base_revision": BASE_REVISION,
        "source_revision": git("rev-parse", "HEAD").strip(),
        "revision": "base revision plus the measured changes fingerprinted per file below",
        "upstream_revision": "30178c4f58b67f8472901356e1484022bd835de0",
        "platform": "aarch64-apple-darwin", "features": "default; no additional features",
        "scope": "Focused tests only; these are not workspace coverage results.",
        "baseline": None, "excluded_file_regex": IGNORE,
        "unavailable": ["Linux", "Windows", "authenticated or paid Provider integration"],
        "measurement": {"selected_crate_files": total(files),
                        "changed_production_files": total(changed_files),
                        "added_or_changed_executable_lines": total(file["changed_lines"] for file in files)},
        "crate_totals": {crate: total(file for file in files if file["crate"] == crate)
                         for crate, _, _ in SCOPES},
        "tests": results, "files": files,
        "changed_rust_sources_sha256": {
            path: hashlib.sha256((ROOT / path).read_bytes()).hexdigest()
            for path in sorted(changed) if path.endswith(".rs") and (ROOT / path).exists()
        },
        "changed_rust_files_without_executable_lines": [
            path for path in sorted(changed)
            if path.endswith(".rs") and not re.search(IGNORE, "/" + path)
            and path not in {file["path"] for file in files}
        ],
        "lcov_sha256": hashlib.sha256(raw.read_bytes()).hexdigest(),
        "toolchain": subprocess.check_output(["rustc", "-Vv"], cwd=ROOT, text=True).strip(),
        "uncovered_or_unverified": [
            "Some persistence/queue failure branches and OS process-termination failure retention are not exercised.",
            "Real authenticated Codex/Claude inference and live GitHub/GHES network/push behavior are not exercised.",
            "Linux and Windows platform branches are not compiled in this macOS measurement.",
            "Global session event producers and conservative ordinary Agent failure retries remain compatibility gaps; see the main audit.",
        ],
    }
    REPORT.mkdir(parents=True, exist_ok=True)
    (REPORT / "coverage.json").write_text(json.dumps(artifact, indent=2, ensure_ascii=False) + "\n")
    summary = artifact["measurement"]
    paragraphs = [
        "# Paseo server 定向覆盖率证据", "",
        "测量基线为 `" + artifact["base_revision"] + "` 加本次修改。",
        "`coverage.json` 保存每个测量源文件的 SHA-256、未覆盖行、修改行命中数及测试命令。",
        "本次没有可比较的旧覆盖率基线；此处保留本地迭代定向测量。",
        "提交前完整 workspace 测量另见[PR 验证报告](../paseo-server-pr-validation-2026-09-29.md)。", "",
        "## Test coverage", "", "| 测量口径 | 覆盖率 | 已覆盖 / 总行数 |", "| --- | ---: | ---: |",
    ]
    for label, key in [("选定 crate 的全部已插桩生产文件", "selected_crate_files"),
                       ("本次改动的生产文件（整个文件）", "changed_production_files"),
                       ("新增或修改的可执行行", "added_or_changed_executable_lines")]:
        value = summary[key]
        paragraphs.append(f"| {label} | {value['percent']}% | {value['covered']} / {value['total']} |")
    paragraphs += ["", "不是逐接口语义覆盖率，也不是 workspace 覆盖率。macOS arm64，默认 features。",
                   "测试文件和 test_support 被排除；不可执行的类型定义、注释及未编译的平台代码不进入 LLVM 行分母。",
                   f"Linux/Windows 和真实认证 Provider 未运行。{sum(result['ignored'] for result in results)} 个既有在线 Provider 测试保持 ignored。", "",
                   "复现：`python3 scripts/paseo-focused-coverage.py --run`。该脚本只运行下面列出的定向测试。",
                   "HTML 位于 `target/llvm-cov/html/index.html`；可评审的文件/行证据为 [coverage.json](coverage.json)。", "",
                   "| 测试目标 | 通过 | ignored | 覆盖率 | 已覆盖 / 总行数 |", "| --- | ---: | ---: | ---: | ---: |"]
    for result in results:
        value = artifact["crate_totals"][result["crate"]]
        paragraphs.append(f"| {result['crate']} | {result['passed']} | {result['ignored']} | {value['percent']}% | {value['covered']} / {value['total']} |")
    paragraphs += ["", "测试数与覆盖率分开统计；每个测试目标只统计此次干净测量中的一次执行。", "",
                   "## 重要未覆盖行为", "",
                   "部分持久化/队列故障分支，以及操作系统拒绝终止进程后保留清理责任的分支，仍需故障注入验证。",
                   "真实 Codex/Claude 认证推理、GitHub/GHES 网络和 push、Linux/Windows 需在相应环境另行验证。",
                   "全局 session event 生产者和普通 Agent 失败重试等剩余兼容差异见[主报告](../paseo-api-audit-2026-09-29.md)。",
                   "定向测试只覆盖改动及直接相关行为，未为提高比例运行其他未改动模块；因此所选 crate 的完整文件分母仍包含未执行的旧路径。", "",
                   "## 精确命令", "", "```sh", "cargo llvm-cov clean --workspace"]
    paragraphs += [" ".join(result["command"]) for result in results]
    paragraphs += [f"cargo llvm-cov report --lcov --output-path target/paseo-focused-coverage/coverage.lcov --ignore-filename-regex '{IGNORE}'",
                   f"cargo llvm-cov report --html --ignore-filename-regex '{IGNORE}'", "```", "",
                   "## 改动文件", "", "| 文件 | 已覆盖 / 总行数 | 新增修改行：已覆盖 / 总行数 |", "| --- | ---: | ---: |"]
    for file in changed_files:
        diff = file["changed_lines"]
        paragraphs.append(f"| [{file['path']}](../../../../{file['path']}) | {file['covered']} / {file['total']} | {diff['covered']} / {diff['total']} |")
    (REPORT / "README.md").write_text("\n".join(paragraphs) + "\n")
    print(json.dumps(summary, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", action="store_true", help="clean and run only the listed focused tests")
    arguments = parser.parse_args()
    if arguments.run:
        run()
    report()
