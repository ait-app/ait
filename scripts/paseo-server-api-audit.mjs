// Run: node scripts/paseo-server-api-audit.mjs /path/to/paseo [--check]
// Load the real Zod union, independently of the Rust catalog, and retain every input schema.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { execFileSync } from "node:child_process";

const root = path.resolve(process.argv[2] ?? "../paseo");
const check = process.argv.includes("--check");
const require = createRequire(path.join(root, "package.json"));
const { tsImport } = require("tsx/esm/api");
const { z } = require("zod");
const revision = execFileSync("git", ["-C", root, "rev-parse", "HEAD"], {
  encoding: "utf8",
}).trim();
assert.equal(revision, "30178c4f58b67f8472901356e1484022bd835de0");
const { SessionInboundMessageSchema } = await tsImport(
  path.join(root, "packages/protocol/src/messages.ts"),
  import.meta.url,
);
const sha = (text) => crypto.createHash("sha256").update(text).digest("hex");
const methods = new Map(
  Object.entries(
    JSON.parse(execFileSync("python3", ["scripts/rust_method_specs.py"], { encoding: "utf8" })),
  ),
);
// Keep pinned upstream mappings in the audit snapshot, outside the runtime catalog.
const fixture = "scripts/fixtures/paseo/paseo-api-contracts.json";
const snapshot = JSON.parse(fs.readFileSync(fixture, "utf8"));
assert.equal(snapshot.source.revision, revision);
const mapping = new Map(
  snapshot.entries
    .filter((entry) => !entry.excluded)
    .map(({ name, kind, group, canonical }) => {
      assert.equal(methods.get(canonical), kind, `Ait method drifted: ${canonical}`);
      return [name, { kind, group, canonical }];
    }),
);
const excluded = new Set(
  fs
    .readFileSync("scripts/fixtures/paseo/excluded-inbound.txt", "utf8")
    .split("\n")
    .filter((line) => line && !line.startsWith("#")),
);
const files = (directory, cwd = process.cwd()) =>
  execFileSync("rg", ["--files", directory], { cwd, encoding: "utf8" }).trim().split("\n").sort();
const rust = [...files("crates"), ...files("bins")]
  .filter((file) => file.endsWith(".rs"))
  .map((file) => ({ file, text: fs.readFileSync(file, "utf8") }));
const protocol = files("packages/protocol/src", root)
  .filter((file) => file.endsWith(".ts") && !file.includes(".test."))
  .map((file) => ({ file, text: fs.readFileSync(path.join(root, file), "utf8") }));
const sessionFile = "packages/server/src/server/session.ts";
const session = fs.readFileSync(path.join(root, sessionFile), "utf8");
const location = (file, text, needle) => {
  const offset = text.indexOf(needle);
  return offset < 0 ? null : { file, line: text.slice(0, offset).split("\n").length };
};
const notes = new Map([
  ["workspace.list.request", "修复 keyset 分页、过滤、状态聚合、sync 与连接所有权订阅"],
  ["agent.list.request", "修复 keyset 分页、sync 与连接所有权订阅；同步包含原生权限状态"],
  ["project.list.request", "修复全量/增量同步及删除序列"],
  ["agent.history.get.request", "修复 keyset 分页及上游多字段模糊搜索"],
  ["agent.timeline.get.request", "修复完整时间线投影、分页和连续确认游标"],
  ["agent.timeline.search.request", "修复按相同投影搜索并定位 seqEnd"],
  ["agent.fork_context.request", "修复投影边界验证、工具摘要和子任务日志"],
  [
    "agent.finish.wait.request",
    "修复无限/长超时、审批、未知身份内联错误及自动归档期间保留最后回复",
  ],
  ["provider.features.list.request", "修复 draftConfig 输入和当前草稿的功能值；失败内联返回"],
  ["provider.models.list.request", "修复隐藏模型过滤与全局目录作用域"],
  ["provider.snapshot.get.request", "修复全局目录作用域及 cwd 规范化"],
  ["provider.snapshot.refresh.request", "修复全局目录作用域及 cwd 规范化"],
  ["provider.modes.list.request", "修复全局目录作用域及 cwd 规范化"],
  [
    "agent.resume.request",
    "修复 overrides、显式归档恢复、未知原生 handle 直接恢复及 cwd 覆盖后继续输入",
  ],
  [
    "agent.create.request",
    "补齐 env、caller、worktree、原目录 Git、autoArchive、setup、GitHub PR checkout 和操作期间实时进度",
  ],
  [
    "workspace.create.request",
    "补齐初始 Agent、共享回执、断线继续、并发去重、安全重试、身份冲突与完成回执验证；GitHub PR checkout 可用",
  ],
  ["creation.subscribe.request", "连接所有权观察、断线恢复；完成回执查询校验目录和 Agent 仍存在"],
  ["terminal.list.request", "补齐 hook activity 投影、退出撤销及 Workspace 状态聚合"],
  ["terminal.list.subscribe.request", "活动变化进入目录事件；终端归属按 Workspace 身份隔离"],
  ["session.heartbeat", "补齐有效可见终端焦点的 attention 清除与通知抑制"],
  [
    "workspace.archive.request",
    "修复 Agent、终端、setup/script 清理及共享 checkout 保留和失败重试",
  ],
  ["project.remove.request", "修复全部所属 Workspace 的资源清理，支持归档记录重试"],
  [
    "workspace.worktree.archive.request",
    "分阶段归档、关闭资源、复核活动引用后删除；返回真实 removedAgents",
  ],
  [
    "workspace.worktree.create.request",
    "修复 checkout 默认目录名及比较基线；支持 GitHub/GHES PR、fork 推送配置与 setup 信任门控",
  ],
  ["agent.timeline.append.request", "仅接受带受信插件来源的显示项；不恢复已移除的 Plugin 产品能力"],
  ["daemon.get_pairing_offer.request", "按 ADR-061 保留禁用配对/relay 的响应"],
  ["editor.open.request", "与当前上游一致：提示改由桌面端打开编辑器"],
  [
    "session.events.set_subscription.request",
    "补齐 terminal attention、审批及原生子 Agent 事件；checkout、script/setup 等事件类别未全部实现",
  ],
]);
function assessment(canonical, group) {
  if (notes.has(canonical)) return notes.get(canonical);
  if (group === "Provider") return "核对输入与路由；仅安装 Codex/Claude，外部账号服务未做真实调用";
  if (group === "Git" && /forge|github|\.pr\./.test(canonical))
    return "核对输入与路由；当前仅 GitHub/GHES，GitLab/Gitea 尚未接入";
  if (group === "Browser") return "核对输入与路由；由登记的浏览器 host 执行命令";
  if (group === "Voice") return "核对输入与路由；使用 Ait 本地语音后端，见 ADR-064";
  return "名称、输入字段与实现入口已核对；语义证据见关联测试及主报告";
}
const entries = SessionInboundMessageSchema.options
  .map((option) => {
    const name = option.shape.type.value;
    const mapped = mapping.get(name);
    assert(mapped || excluded.has(name), `Unmapped upstream method: ${name}`);
    assert(!(mapped && excluded.has(name)), `Excluded method was reintroduced: ${name}`);
    const schema = z.toJSONSchema(option, { unrepresentable: "any", io: "input" });
    const schemaSources = protocol
      .filter((source) => source.text.includes(`"${name}"`))
      .map((source) => location(source.file, source.text, `"${name}"`));
    const handlers = mapped
      ? rust
          .filter(
            (source) =>
              !source.file.includes("/tests") &&
              /\/(rpc|dispatch|connection|service)(\/|\.)/.test(source.file),
          )
          .map((source) => location(source.file, source.text, `"${mapped.canonical}"`))
          .filter(Boolean)
      : [];
    const tests = mapped
      ? rust
          .filter(
            (source) =>
              source.file.includes("/tests") &&
              (source.text.includes(`"${mapped.canonical}"`) || source.text.includes(`"${name}"`)),
          )
          .map((source) => source.file)
      : [];
    return {
      name,
      ...mapped,
      excluded: excluded.has(name),
      schemaSources,
      upstreamDispatch: location(sessionFile, session, `case "${name}"`),
      handlers,
      tests,
      assessment: mapped
        ? assessment(mapped.canonical, mapped.group)
        : "已按 ADR-045/047/061 移除，不重新接入",
      schemaSha256: sha(JSON.stringify(schema)),
      schema,
    };
  })
  .sort((a, b) => a.name.localeCompare(b.name, "en"));
assert.equal(entries.length, 205);
assert.equal(entries.filter((entry) => entry.excluded).length, 34);
assert.equal(
  new Set(entries.filter((entry) => !entry.excluded).map((entry) => entry.canonical)).size,
  168,
);
const source = {
  revision,
  packageVersion: JSON.parse(
    fs.readFileSync(path.join(root, "packages/server/package.json"), "utf8"),
  ).version,
  schemaFiles: Object.fromEntries(protocol.map((file) => [file.file, sha(file.text)])),
  sessionSha256: sha(session),
  note: "Input schemas are extracted from upstream. Handler/test links are an index, not a claim of complete behavioral equivalence.",
};
function write(destination, content) {
  if (check)
    assert.equal(
      fs.readFileSync(destination, "utf8"),
      content,
      `${destination} drifted; inspect before regeneration`,
    );
  else fs.writeFileSync(destination, content);
}
write(fixture, `${JSON.stringify({ source, entries }, null, 2)}\n`);
const reference = (value) => (value ? `\`${value.file}:${value.line}\`` : "—");
const rows = entries.map((entry) => {
  const fields =
    Object.keys(entry.schema.properties)
      .filter((field) => !["type", "requestId"].includes(field))
      .join(", ") || "—";
  const handlers =
    entry.handlers.slice(0, 2).map(reference).join("<br>") || "见对应 capability 分派";
  return `| \`${entry.name}\` | ${entry.canonical ? `\`${entry.canonical}\`` : "已移除"} | ${fields} | ${entry.excluded ? "—" : handlers} | ${entry.assessment} |`;
});
write(
  "docs/reports/daemon/paseo-api-matrix-2026-09-29.md",
  `# Paseo Server 逐接口索引\n\n由 \`scripts/paseo-server-api-audit.mjs\` 从本地 Paseo \`${revision}\` 的真实 Zod union 生成。\n205 个入站名称，34 个已明确移除，171 个有效名称归并为 168 个 canonical 方法。\n完整嵌套字段、每项 schema 指纹、上游分派位置和关联 Rust 测试保存在\n[契约快照](../../../${fixture})；测试索引不是逐项语义覆盖率。\n\n[修复、验证与未覆盖范围](paseo-api-audit-2026-09-29.md)。表格不以“有路由”推断完全兼容。\n\n| Paseo 入站名称 | Ait 方法 | 上游输入字段 | Rust 入口 | 对照结果 / 限制 |\n| --- | --- | --- | --- | --- |\n${rows.join("\n")}\n`,
);
console.log(
  JSON.stringify({
    entries: entries.length,
    scoped: entries.length - excluded.size,
    canonical: 168,
    fixture,
    check,
  }),
);
