#!/usr/bin/env node
// Offline metadata process peer; all state remains in the per-test working directory.
const fs = require("node:fs");
const args = process.argv.slice(2);
if (args.includes("--dump-config")) {
  console.log(fs.readFileSync(process.env.AIT_METADATA_FIXTURE_CONFIG, "utf8"));
} else {
  const patch = JSON.parse(fs.readFileSync(args[args.indexOf("--patch") + 1], "utf8"));
  fs.writeFileSync("observed.json", JSON.stringify(patch));
  console.log('{"title":"fixture summary"}');
}
