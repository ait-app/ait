const { withMainApplication } = require("expo/config-plugins");

const MARKER = "// Ait native account relay WebSockets";
const SETUP = `    ${MARKER}
    com.facebook.react.modules.websocket.WebSocketModule.setCustomClientBuilder { builder ->
      builder.followRedirects(false).followSslRedirects(false)
      builder.addInterceptor { chain ->
        val request = chain.request()
        val relayPath = Regex(".*/v1/relay/sessions/[0-9a-fA-F-]{36}/client")
        val nativeRelay = request.header("Authorization")?.startsWith("Bearer ") == true &&
          relayPath.matches(request.url.encodedPath)
        // Native tickets are issued without a browser Origin binding.
        chain.proceed(if (nativeRelay) request.newBuilder().removeHeader("Origin").build() else request)
      }
    }
`;

function configureAccountRelayWebSocket(contents) {
  if (contents.includes(MARKER)) return contents;
  const anchor = "    super.onCreate()\n";
  if (!contents.includes(anchor))
    throw new Error("Could not configure native account relay WebSockets");
  return contents.replace(anchor, `${anchor}${SETUP}`);
}

module.exports = (config) =>
  withMainApplication(config, (mod) => {
    if (mod.modResults.language !== "kt")
      throw new Error("Account relay requires a Kotlin MainApplication");
    mod.modResults.contents = configureAccountRelayWebSocket(mod.modResults.contents);
    return mod;
  });
module.exports.configureAccountRelayWebSocket = configureAccountRelayWebSocket;
