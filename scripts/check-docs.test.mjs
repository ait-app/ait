import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import {
  LEGACY_DUPLICATE_NUMBERS,
  checkDecisions,
  governing,
  parseFrontMatter,
  readDecisions,
  renderIndex,
  repositoryPath,
} from "./adr.mjs";
import { checkDocumentationLinks } from "./check-docs.mjs";

test("checks local links and images without interpreting examples or remote URLs", (t) => {
  const root = mkdtempSync(path.join(tmpdir(), "ait-doc-links-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(path.join(root, "guide"));
  writeFileSync(path.join(root, "guide", "present file.md"), "# Present");
  const file = path.join(root, "README.md");
  writeFileSync(
    file,
    [
      "[Present](guide/present%20file.md#present)",
      "[Directory](guide)",
      "[External](https://example.com/missing.md)",
      "[Anchor](#heading)",
      "```markdown\n[Example](missing-example.md)\n```",
      "[Missing](missing.md)",
      "![Missing image](missing.png)",
    ].join("\n"),
  );
  assert.deepEqual(checkDocumentationLinks([file]), [
    `${file}: missing local link missing.md`,
    `${file}: missing local link missing.png`,
  ]);
});

function decisionTree(t, files) {
  const root = mkdtempSync(path.join(tmpdir(), "ait-adr-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const [file, text] of Object.entries(files)) {
    const target = path.join(root, "docs", "decisions", file);
    mkdirSync(path.dirname(target), { recursive: true });
    writeFileSync(target, text);
  }
  return root;
}

const adr = (number, fields, title = `ADR-${number}：Title`) =>
  `---\n${fields}\n---\n\n# ${title}\n\n## 背景\n`;
const accepted = (number, extra = "") =>
  adr(
    number,
    `id: ADR-${number}\nstatus: accepted\ndate: 2026-11-10\n${extra}paths:\n  - crates/bot`,
  );
const ownedPaths = (owned) => owned === "crates/bot";

test("parses block and flow lists, including flow lists the formatter wraps", () => {
  const parsed = parseFrontMatter(
    '---\nid: ADR-001\namends:\n  - "bot/adr-001-a.md"\npaths:\n  [crates/bot, apps/mobile]\n---\n\n# x',
  );
  assert.deepEqual(parsed.fields, {
    id: "ADR-001",
    amends: ["bot/adr-001-a.md"],
    paths: ["crates/bot", "apps/mobile"],
  });
  assert.deepEqual(parsed.problems, []);
  assert.equal(parseFrontMatter("# ADR-001：No metadata"), null);
});

test("ADR metadata needs known statuses, required fields, real targets and owning paths", (t) => {
  const root = decisionTree(t, {
    "bot/adr-300-good.md": accepted("300", "amends:\n  - bot/adr-301-bad.md\n"),
    "bot/adr-301-bad.md": adr(
      "301",
      "id: ADR-301\nstatus: Accepted\ndate: 2026-13-01\nsupersedes:\n  - bot/adr-999-gone.md\npaths:\n  - crates/missing",
    ),
    "bot/adr-302-partial.md": adr("302", "id: ADR-303\nstatus: accepted"),
    "bot/adr-303-plain.md": "# ADR-303：Old header style\n\n- 状态：接受\n",
  });
  const problems = checkDecisions(readDecisions(root), { exists: ownedPaths });
  const decisions = "docs/decisions/bot";
  assert.deepEqual(problems, [
    `${decisions}/adr-303-plain.md: missing front matter`,
    `${decisions}/adr-302-partial.md: missing required field date`,
    `${decisions}/adr-302-partial.md: missing required field paths`,
    `${decisions}/adr-302-partial.md: id ADR-303 does not match file number 302`,
    `${decisions}/adr-301-bad.md: status Accepted is not one of proposed, accepted, superseded, withdrawn`,
    `${decisions}/adr-301-bad.md: date 2026-13-01 is not YYYY-MM-DD`,
    `${decisions}/adr-301-bad.md: path crates/missing does not exist`,
    `${decisions}/adr-301-bad.md: supersedes target bot/adr-999-gone.md does not exist`,
  ]);
});

test("superseded status and supersedes links must agree", (t) => {
  const root = decisionTree(t, {
    "bot/adr-300-old.md": accepted("300"),
    "bot/adr-301-new.md": accepted("301", "supersedes:\n  - bot/adr-300-old.md\n"),
    "bot/adr-302-orphan.md": adr(
      "302",
      "id: ADR-302\nstatus: superseded\ndate: 2026-11-10\npaths:\n  - crates/bot",
    ),
  });
  assert.deepEqual(checkDecisions(readDecisions(root), { exists: ownedPaths }), [
    "docs/decisions/bot/adr-302-orphan.md: status is superseded but no ADR lists it in supersedes",
    "docs/decisions/bot/adr-300-old.md: superseded by bot/adr-301-new.md but status is accepted",
  ]);
});

test("only the recorded legacy numbers may repeat, and only twice", (t) => {
  assert.equal(LEGACY_DUPLICATE_NUMBERS.size, 16);
  const root = decisionTree(t, {
    "bot/adr-074-a.md": accepted("074"),
    "daemon/adr-074-b.md": accepted("074"),
    "bot/adr-180-a.md": accepted("180"),
    "daemon/adr-180-b.md": accepted("180"),
    "providers/adr-180-c.md": accepted("180"),
    "bot/adr-300-a.md": accepted("300"),
    "clients/adr-300-b.md": accepted("300"),
  });
  assert.deepEqual(checkDecisions(readDecisions(root), { exists: ownedPaths }), [
    "docs/decisions: ADR-300 is used by 2 files (bot/adr-300-a.md, clients/adr-300-b.md); pick an unused number",
    "docs/decisions: ADR-180 is used by 3 files (bot/adr-180-a.md, daemon/adr-180-b.md, providers/adr-180-c.md); pick an unused number",
  ]);
});

test("the index keeps its introduction, annotates relations and must be regenerated", (t) => {
  const root = decisionTree(t, {
    "bot/adr-300-old.md": adr(
      "300",
      "id: ADR-300\nstatus: superseded\ndate: 2026-11-01\npaths:\n  - crates/bot",
    ),
    "bot/adr-301-base.md": accepted("301"),
    "bot/adr-302-new.md": accepted(
      "302",
      "supersedes:\n  - bot/adr-300-old.md\namends:\n  - bot/adr-301-base.md\n",
    ),
  });
  const decisions = readDecisions(root);
  const index = renderIndex(decisions, "# Index\n\nIntro.\n\n## Bot\n\n- stale\n");
  assert.match(
    index,
    /^# Index\n\nIntro\.\n\n## Bot\n\n- \[ADR-302：Title\]\(bot\/adr-302-new\.md\)\n/,
  );
  assert.match(index, /\(bot\/adr-301-base\.md\)（经 \[ADR-302\]\(bot\/adr-302-new\.md\) 修订）/);
  assert.match(index, /\(bot\/adr-300-old\.md\)（已由 \[ADR-302\]\(bot\/adr-302-new\.md\) 取代）/);
  assert.deepEqual(checkDecisions(decisions, { exists: ownedPaths, index }), []);
  assert.deepEqual(checkDecisions(decisions, { exists: ownedPaths, index: "# Index\n" }), [
    "docs/decisions/README.md: out of date; run node scripts/adr.mjs index to regenerate it",
  ]);
});

test("governing ADRs match by path prefix in both directions and skip replaced ones", (t) => {
  const root = decisionTree(t, {
    "bot/adr-300-old.md": adr(
      "300",
      "id: ADR-300\nstatus: superseded\ndate: 2026-11-01\npaths:\n  - crates/bot",
    ),
    "bot/adr-301-new.md": accepted("301", "supersedes:\n  - bot/adr-300-old.md\n"),
    "clients/adr-302-app.md": adr(
      "302",
      "id: ADR-302\nstatus: accepted\ndate: 2026-11-10\npaths:\n  - apps/mobile/src/bots",
    ),
  });
  const decisions = readDecisions(root);
  const files = (target, options) => governing(decisions, target, options).map((d) => d.file);
  assert.deepEqual(files("crates/bot/src/lib.rs"), ["bot/adr-301-new.md"]);
  assert.deepEqual(files("crates/bot", { all: true }), [
    "bot/adr-301-new.md",
    "bot/adr-300-old.md",
  ]);
  assert.deepEqual(files("apps/mobile"), ["clients/adr-302-app.md"]);
  assert.deepEqual(files("crates/botanist"), []);
  assert.equal(repositoryPath("./crates/bot/", { cwd: root, root }), "crates/bot");
});
