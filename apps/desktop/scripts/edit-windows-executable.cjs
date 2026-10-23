const fs = require("node:fs");
const { NtExecutable, NtExecutableResource, Resource, Data } = require("resedit");

function editWindowsExecutable({ executablePath, iconPath, version }) {
  const executable = NtExecutable.from(fs.readFileSync(executablePath));
  const resources = NtExecutableResource.from(executable);
  const versionInfo = Resource.VersionInfo.fromEntries(resources.entries)[0];
  if (!versionInfo) throw new Error("Windows executable is missing version information");
  const numericVersion = version.split("-")[0];
  versionInfo.setFileVersion(numericVersion, 1033);
  versionInfo.setProductVersion(numericVersion, 1033);
  versionInfo.setStringValues(
    { lang: 1033, codepage: 1200 },
    {
      CompanyName: "Necokeine",
      FileDescription: "Ait",
      ProductName: "Ait",
      InternalName: "Ait",
      OriginalFilename: "Ait.exe",
      ProductVersion: version,
      LegalCopyright: "Copyright © 2026 Necokeine",
    },
  );
  versionInfo.outputToResourceEntries(resources.entries);
  const icons = Data.IconFile.from(fs.readFileSync(iconPath)).icons.map((icon) => icon.data);
  for (const group of Resource.IconGroupEntry.fromEntries(resources.entries)) {
    Resource.IconGroupEntry.replaceIconsForResource(resources.entries, group.id, group.lang, icons);
  }
  resources.outputResource(executable);
  fs.writeFileSync(executablePath, Buffer.from(executable.generate()));
}

module.exports = { editWindowsExecutable };
