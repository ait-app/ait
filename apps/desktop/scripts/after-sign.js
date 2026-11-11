const path = require("node:path");
const { execFileSync } = require("node:child_process");

const EXECUTABLE_NAME = "Ait";

function verifyMacSignatures(appOutDir, run = execFileSync) {
  const app = path.join(appOutDir, `${EXECUTABLE_NAME}.app`);
  for (const name of ["aitd", "ait-omp-acp"]) {
    run(
      "codesign",
      ["--verify", "--strict", "--verbose=2", path.join(app, "Contents/Resources/bin", name)],
      { stdio: "inherit" },
    );
  }
  run("codesign", ["--verify", "--deep", "--strict", "--verbose=2", app], { stdio: "inherit" });
}

exports.verifyMacSignatures = verifyMacSignatures;

exports.default = async function afterSign(context) {
  if (context.electronPlatformName !== "darwin") return;
  verifyMacSignatures(context.appOutDir);
  if (process.env.AIT_DESKTOP_SMOKE !== "1") return;

  execFileSync(process.execPath, [path.join(__dirname, "../e2e/rust-startup.e2e.mjs")], {
    stdio: "inherit",
    env: {
      ...process.env,
      AIT_PACKAGED_APP: path.join(context.appOutDir, `${EXECUTABLE_NAME}.app`),
    },
  });
};
