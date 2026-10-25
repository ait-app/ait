const path = require("node:path");
const { sourceVersion, packagedVersion } = require("../../../scripts/source-version.cjs");
const { version } = require("../package.json");

const displayVersion = sourceVersion(version, path.resolve(__dirname, "../../.."));

module.exports = {
  extraMetadata: { version: packagedVersion(displayVersion), displayVersion },
};
