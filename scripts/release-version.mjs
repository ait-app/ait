export function releaseChannel(version) {
  if (
    typeof version !== "string" ||
    version !== version.trim() ||
    !/^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-beta\.[1-9]\d*)?$/.test(version)
  ) {
    throw new Error(`Release version must use X.Y.Z or X.Y.Z-beta.N: ${version}`);
  }
  return version.includes("-beta.") ? "beta" : "latest";
}
