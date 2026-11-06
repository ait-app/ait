const path = require("node:path");
const { pathToFileURL } = require("node:url");

const ARCH_MAP = { 1: "x64", 3: "arm64" };

exports.default = async function beforePack(context) {
  const arch = ARCH_MAP[context.arch];
  if (!arch)
    throw new Error(`OMP native host does not support Electron architecture ${context.arch}`);
  const { installRuntime } = await import(
    pathToFileURL(path.resolve(__dirname, "../../../runtimes/omp-native-host/install.mjs")).href
  );
  await installRuntime({
    destination: path.resolve(
      __dirname,
      "../release-resources/provider-runtimes/omp-native-host/1",
    ),
    platform: context.electronPlatformName,
    arch,
  });
  if (context.electronPlatformName === "darwin") {
    const options = context.packager.platformSpecificBuildOptions;
    const prefix = "Contents/Resources/provider-runtimes/omp-native-host/1/node_modules/@oven/";
    const cpu = arch === "arm64" ? "aarch64" : "x64";
    options.binaries = [
      ...(options.binaries ?? []).filter((binary) => !binary.startsWith(prefix)),
      `${prefix}bun-darwin-${cpu}/bin/bun`,
    ];
  }
};
