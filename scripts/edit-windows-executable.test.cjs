const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { NtExecutable, NtExecutableResource, Resource, Data } = require("resedit");
const { editWindowsExecutable } = require("../apps/desktop/scripts/edit-windows-executable.cjs");

test("cross-built Windows executables retain integrity while receiving Ait identity and icons", (t) => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "ait-windows-exe-"));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const executablePath = path.join(directory, "Ait.exe");
  const iconPath = path.resolve(__dirname, "../apps/desktop/assets/icon.ico");
  const icons = Data.IconFile.from(fs.readFileSync(iconPath)).icons.map((icon) => icon.data);
  const executable = NtExecutable.createEmpty(false, false);
  const resources = NtExecutableResource.from(executable);
  const version = Resource.VersionInfo.createEmpty();
  version.setFileVersion("44.2.0", 1033);
  version.outputToResourceEntries(resources.entries);
  Resource.IconGroupEntry.replaceIconsForResource(resources.entries, 1, 1033, icons.slice(0, 1));
  resources.entries.push({
    type: "INTEGRITY",
    id: "ELECTRONASAR",
    lang: 1033,
    codepage: 1200,
    bin: Buffer.from("integrity-must-survive"),
  });
  resources.outputResource(executable);
  fs.writeFileSync(executablePath, Buffer.from(executable.generate()));

  editWindowsExecutable({ executablePath, iconPath, version: "0.1.15-beta.1" });

  const result = NtExecutableResource.from(NtExecutable.from(fs.readFileSync(executablePath)));
  const strings = Resource.VersionInfo.fromEntries(result.entries)[0].getStringValues({
    lang: 1033,
    codepage: 1200,
  });
  assert.equal(strings.ProductName, "Ait");
  assert.equal(strings.OriginalFilename, "Ait.exe");
  assert.equal(strings.FileVersion, "0.1.15.0");
  assert.equal(strings.ProductVersion, "0.1.15-beta.1");
  const group = Resource.IconGroupEntry.fromEntries(result.entries)[0];
  assert.equal(group.icons.length, icons.length);
  assert.equal(
    Buffer.from(result.entries.find((entry) => entry.type === "INTEGRITY").bin).toString(),
    "integrity-must-survive",
  );
});
