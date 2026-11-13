$ErrorActionPreference = "Stop"
$RootDir = (Resolve-Path "$PSScriptRoot\..\..\..").Path
$env:PATH = "$RootDir\node_modules\.bin;$env:PATH"
install-electron
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if (!$env:AIT_SERVER_DATA_DIR) { $env:AIT_SERVER_DATA_DIR = "$RootDir\.tmp\ait\server" }
if (!$env:AIT_ELECTRON_USER_DATA_DIR) { $env:AIT_ELECTRON_USER_DATA_DIR = "$RootDir\.tmp\ait\electron" }
if (!$env:EXPO_PORT) { $env:EXPO_PORT = (get-port 8082 8083 8084 8085).Trim() }
$env:EXPO_DEV_URL = "http://localhost:$env:EXPO_PORT"
if (!$env:AITD_BIN) {
    cargo build --manifest-path "$RootDir\Cargo.toml" --target-dir "$RootDir\target" -p daemon --bin daemon
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $env:AITD_BIN = "$RootDir\target\debug\daemon.exe"
}
npm --prefix "$RootDir" run build:desktop-main
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
node "$PSScriptRoot\dev-runner.mjs" @args
exit $LASTEXITCODE
