module.exports = function (api) {
  api.cache(true);

  const expoPreset = [
    "babel-preset-expo",
    {
      // Transform `import.meta` for ALL platforms (web + native)
      // Required for modern ESM deps like Zustand 5 that use import.meta.env
      unstable_transformImportMeta: true,
      // Protocol schemas and generated validators contain no React components. Their large
      // control-flow graphs must not delay desktop startup or every protocol hot reload.
      "react-compiler": {
        sources: (filename) => !filename.replaceAll("\\", "/").includes("/packages/protocol/"),
      },
    },
  ];

  return {
    presets: [expoPreset],
    plugins: [
      [
        "react-native-unistyles/plugin",
        {
          root: "src",
        },
      ],
    ],
  };
};
