const path = require("node:path");
const { execFileSync } = require("node:child_process");

const EXECUTABLE_NAME = "Ait";

exports.default = async function afterSign(context) {
  if (process.env.AIT_DESKTOP_SMOKE !== "1") {
    return;
  }

  if (context.electronPlatformName !== "darwin") {
    return;
  }

  execFileSync(process.execPath, [path.join(__dirname, "../e2e/rust-startup.e2e.mjs")], {
    stdio: "inherit",
    env: {
      ...process.env,
      AIT_PACKAGED_APP: path.join(context.appOutDir, `${EXECUTABLE_NAME}.app`),
    },
  });
};
