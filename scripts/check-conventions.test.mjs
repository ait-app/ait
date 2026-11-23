import assert from "node:assert/strict";
import test from "node:test";
import {
  checkRustLayout,
  compareWaits,
  countRealTimeWaits,
  loweredBaseline,
  waitPattern,
} from "./check-conventions.mjs";

test("only test files are scanned for real-time waits", () => {
  for (const file of [
    "crates/bot/src/follow/tests.rs",
    "crates/bot/src/follow/tests/lease.rs",
    "bins/daemon/tests/process/terminal.rs",
    "crates/provider/runtime/src/testing/agent_mcp.rs",
    "crates/api/src/test_support.rs",
    "apps/mobile/src/usage/display.test.ts",
    "apps/mobile/e2e/browser/chat-find.spec.ts",
    "scripts/verify.test.mjs",
  ]) {
    assert.notEqual(waitPattern(file), null, file);
  }
  for (const file of [
    "crates/bot/src/follow.rs",
    "crates/bot/src/contests.rs",
    "apps/mobile/src/usage/display.ts",
    "scripts/check-conventions.test.mjs",
  ]) {
    assert.equal(waitPattern(file), null, file);
  }
});

test("Rust waits count sleep calls but not definitions, comments, fixtures or virtual time", () => {
  const source = [
    "tokio::time::sleep(Duration::from_millis(20)).await;",
    "std::thread::sleep(Duration::from_millis(5));",
    "thread::sleep(PAUSE); sleep(PAUSE).await;",
    "() = tokio::time::sleep_until(deadline) => {}",
    "async fn sleep_until(wake: Option<Instant>) {}",
    "// tokio::time::sleep(Duration::from_secs(1)).await;",
    'Fixture::new("receive()\\ntime.sleep(60)");',
    "tokio::time::sleep(Duration::from_secs(5)).await; // virtual time",
    "let slept = asleep(1);",
  ].join("\n");
  assert.equal(countRealTimeWaits(source, waitPattern("crates/x/src/tests.rs")), 5);
});

test("script waits count timer promises, sleep helpers and Playwright timeouts", () => {
  const source = [
    "await new Promise((resolve) => setTimeout(resolve, 10));",
    "const tick = () => new Promise((r) => setTimeout(r, 0));",
    "await new Promise((resolve) => setTimeout(() => resolve(), 5));",
    "await page.waitForTimeout(300);",
    "await sleep(50);",
    "await setTimeout(20);",
    "const timer = setTimeout(onIdle, 30);",
    "vi.advanceTimersByTime(1000); await sleep(1000); // virtual time",
    "  * await sleep(10) in a doc comment",
  ].join("\n");
  assert.equal(countRealTimeWaits(source, waitPattern("apps/x/a.test.ts")), 6);
});

test("the ratchet fails on rises and new files and asks to lower the baseline on drops", () => {
  const baseline = { "a/tests.rs": 2, "b/tests.rs": 3, "gone/tests.rs": 1 };
  const counts = { "a/tests.rs": 3, "b/tests.rs": 1, "new.test.ts": 1 };
  const { regressions, improvements } = compareWaits(baseline, counts);
  assert.deepEqual(regressions, [
    "a/tests.rs: 3 real-time waits in tests, baseline 2; await the event, channel or process exit instead (AGENTS.md)",
    "new.test.ts: 1 real-time waits in tests, baseline 0; await the event, channel or process exit instead (AGENTS.md)",
  ]);
  assert.deepEqual(
    improvements.map((line) => line.split(";")[0]),
    [
      "b/tests.rs: 1 real-time waits in tests, baseline 3",
      "gone/tests.rs: 0 real-time waits in tests, baseline 1",
    ],
  );
  assert.match(improvements[0], /--update to lower scripts\/real-time-waits\.json$/);
  assert.deepEqual(compareWaits({ "a/tests.rs": 2 }, { "a/tests.rs": 2 }), {
    regressions: [],
    improvements: [],
  });
});

test("updating only lowers counts and drops files that reach zero", () => {
  assert.deepEqual(
    loweredBaseline(
      { "b/tests.rs": 3, "a/tests.rs": 2, "gone/tests.rs": 1 },
      { "a/tests.rs": 5, "b/tests.rs": 1, "new.test.ts": 4 },
    ),
    { "a/tests.rs": 2, "b/tests.rs": 1 },
  );
});

test("Rust test modules live in child files and source files stay under 5000 lines", () => {
  const declared = "pub fn add() {}\n\n#[cfg(test)]\nmod tests;\n";
  assert.deepEqual(checkRustLayout("crates/x/src/lib.rs", declared), []);
  // Support modules may group helpers inline; test modules may not.
  assert.deepEqual(checkRustLayout("crates/x/src/test_support.rs", "pub mod bots {}\n"), []);
  const inline = "fn add() {}\n\n#[cfg(test)]\n#[allow(clippy::pedantic)]\nmod checks {\n}\n";
  const nested = "use super::*;\n\nmod fixtures {\n}\n";
  const named = "pub(crate) mod tests {\n}\n";
  const message =
    "inline test module body; declare it with `mod name;` and move it to a child file";
  assert.deepEqual(checkRustLayout("crates/x/src/a.rs", inline), [`crates/x/src/a.rs: ${message}`]);
  assert.deepEqual(checkRustLayout("crates/x/src/a/tests.rs", nested), [
    `crates/x/src/a/tests.rs: ${message}`,
  ]);
  assert.deepEqual(checkRustLayout("crates/x/src/b.rs", named), [`crates/x/src/b.rs: ${message}`]);

  assert.deepEqual(checkRustLayout("crates/x/src/c.rs", "//\n".repeat(5000)), []);
  assert.deepEqual(checkRustLayout("crates/x/src/c.rs", "//\n".repeat(5001)), [
    "crates/x/src/c.rs: 5001 lines exceeds 5000; split it into a module directory",
  ]);
});
