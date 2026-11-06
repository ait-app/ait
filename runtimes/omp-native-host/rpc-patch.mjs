import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

/**
 * Patches the integrity-locked official SDK in place. Every step is verified before it runs,
 * every anchor is checked, and a second run over an already patched tree is a no-op.
 */

const assets = fileURLToPath(new URL(".", import.meta.url));

export const SDK_VERSION = "18.17.0";
export const BRIDGE_VERSION = 1;
const RPC_DIRECTORY = "src/modes/rpc";

/** Exact content of the published 18.17.0 tarball; anything else is refused, never patched. */
export const EXPECTED_SHA256 = Object.freeze({
	"src/modes/rpc/rpc-mode.ts": "3d361daeb3b12c06bb168bf062683bbc117af7962f88c886553689d667307c9f",
	"src/main.ts": "40ec97246510c3eb34e0d1a14687538a372f23321c2136ee6e9f68e4a2f96291",
});

/** Verbatim copies of the Ait adapter sources. The controller imports `./ait-vibe-observer`. */
const COPIES = Object.freeze([
	["vibe-controller.ts", `${RPC_DIRECTORY}/ait-vibe-controller.ts`],
	["vibe-observer.ts", `${RPC_DIRECTORY}/ait-vibe-observer.ts`],
	["ait-native-host-bridge.ts", `${RPC_DIRECTORY}/ait-native-host-bridge.ts`],
]);

const HELLO_MODULE = `${RPC_DIRECTORY}/ait-native-host-hello.ts`;

/** Modules the copied adapter imports; a missing one means the SDK moved and the patch must stop. */
const ADAPTER_IMPORTS = Object.freeze([
	"src/config/model-resolver.ts",
	"src/session/agent-session.ts",
	"src/slash-commands/available-commands.ts",
	"src/vibe/runtime.ts",
	"src/vibe/lifecycle.ts",
	"src/registry/agent-registry.ts",
	"src/session/session-loader.ts",
	`${RPC_DIRECTORY}/rpc-subagents.ts`,
	`${RPC_DIRECTORY}/rpc-btw.ts`,
]);

const HELLO_SOURCE = `/** Ait native host greeting, emitted in \`--mode rpc-ui\` before the official ready frame. */
export const AIT_NATIVE_HOST_HELLO = Object.freeze({
	type: "ait_native_host_hello",
	bridgeVersion: ${BRIDGE_VERSION},
	sdkVersion: "${SDK_VERSION}",
	capabilities: Object.freeze({ vibe: true, vibeWorkerObservation: true, vibeWorkerControl: true }),
});
`;

/** Anchored edits. Each replacement is self-identifying, which makes reapplication a no-op. */
const EDITS = Object.freeze({
	"src/modes/rpc/rpc-mode.ts": [
		{
			find: "\theadless?: boolean;\n\tsubagentEventBus?: EventBus;\n",
			replace: "\t/** Ait native host greeting written before \`ready\`; absent in plain RPC mode. */\n\tnativeHostHello?: object;\n\theadless?: boolean;\n\tsubagentEventBus?: EventBus;\n",
		},
		{
			find: "\t\theadless = false,\n\t\tsubagentEventBus,\n",
			replace: "\t\tnativeHostHello,\n\t\theadless = false,\n\t\tsubagentEventBus,\n",
		},
		{
			find: "\toutputWriter.write(\n\t\tframeEncoder.encodeFrames({\n\t\t\ttype: \"ready\",\n",
			replace: "\t// Ait native host: the bridge greeting precedes the official ready frame.\n\tif (nativeHostHello) outputWriter.write(frameEncoder.encodeFrames(nativeHostHello));\n\toutputWriter.write(\n\t\tframeEncoder.encodeFrames({\n\t\t\ttype: \"ready\",\n",
		},
		{
			find: 'import { RpcBtwController } from "./rpc-btw";\n',
			replace: 'import { RpcBtwController } from "./rpc-btw";\nimport { AitNativeHostBridge } from "./ait-native-host-bridge";\n',
		},
		{
			find: "\tconst btw = new RpcBtwController(session, output);\n",
			replace: "\tconst btw = new RpcBtwController(session, output);\n\t// Ait native host: only a patched \`rpc-ui\` launch owns Vibe state.\n\tconst nativeHostBridge = nativeHostHello ? new AitNativeHostBridge(session, output) : undefined;\n",
		},
		{
			find: "\tconst getAvailableCommands = async () => buildAvailableSlashCommands(session);\n",
			replace: "\t// Ait native host: rehydrate Vibe scopes, then let the catalog advertise \`/vibe\`.\n\tif (nativeHostBridge) await nativeHostBridge.initialize();\n\t// The merged catalog is what input dispatch must search: \`/vibe\` exists only here.\n\tlet nativeHostCommands: Awaited<ReturnType<typeof buildAvailableSlashCommands>> = [];\n\tconst getAvailableCommands = async () => {\n\t\tconst commands = await buildAvailableSlashCommands(session);\n\t\tnativeHostCommands = nativeHostBridge ? nativeHostBridge.commands(commands) : commands;\n\t\treturn nativeHostCommands;\n\t};\n",
		},
		{
			find: "\t\t\tif (!isCurrent()) return \"cancelled\";\n\t\t\tif (!text.trim() && !images?.length) return \"local\";\n",
			replace: "\t\t\tif (!isCurrent()) return \"cancelled\";\n\t\t\tif (nativeHostBridge) {\n\t\t\t\tconst directive = await nativeHostBridge.input(text, nativeHostCommands);\n\t\t\t\tif (!isCurrent()) return \"cancelled\";\n\t\t\t\t// \`null\` only toggled the mode; a string replaces the prompt with its inline directive.\n\t\t\t\tif (directive === null) return \"local\";\n\t\t\t\tif (directive !== undefined) text = directive;\n\t\t\t}\n\t\t\tif (!text.trim() && !images?.length) return \"local\";\n",
		},
		{
			find: "\tconst handleCommandImpl = async (command: RpcCommand): Promise<RpcResponse> => {\n\t\tconst id = command.id;\n",
			replace: "\tconst handleCommandImpl = async (command: RpcCommand): Promise<RpcResponse> => {\n\t\tconst id = command.id;\n\t\t// Ait native host control (list/spawn/send/wait/kill/toggle) over the host action channel.\n\t\tif ((command as { type: string }).type === \"ait_vibe_control\") {\n\t\t\tif (!nativeHostBridge) return error(id, \"ait_vibe_control\", \"The Ait native host bridge is not available.\");\n\t\t\ttry {\n\t\t\t\tconst data = (await nativeHostBridge.control(command as never)) as object;\n\t\t\t\treturn { id, type: \"response\", command: \"ait_vibe_control\", success: true, data } as RpcResponse;\n\t\t\t} catch (controlError) {\n\t\t\t\treturn error(id, \"ait_vibe_control\", toError(controlError).message);\n\t\t\t}\n\t\t}\n",
		},
		{
			find: "\tconst handleCommand = async (command: RpcCommand): Promise<RpcResponse> => {\n",
			replace: "\tconst handleCommand = async (command: RpcCommand): Promise<RpcResponse> => {\n\t\tif (nativeHostBridge) {\n\t\t\ttry {\n\t\t\t\tnativeHostBridge.assertRequest(command as never);\n\t\t\t} catch (veto) {\n\t\t\t\treturn error(command.id, command.type, toError(veto).message);\n\t\t\t}\n\t\t}\n",
		},
		{
			find: "\t\t\t// Close the realtime call (microphone, socket) before the session it delegates into.\n\t\t\tdictationBridge.dispose();\n",
			replace: "\t\t\t// Ait native host: stop Vibe observation before the session it reads is disposed.\n\t\t\tnativeHostBridge?.close();\n\t\t\t// Close the realtime call (microphone, socket) before the session it delegates into.\n\t\t\tdictationBridge.dispose();\n",
		},
	],
	"src/main.ts": [
		{
			find: "\t\t\t\tconst runRpcMode: RunRpcMode = (await import(\"./modes/rpc/rpc-mode\")).runRpcMode;\n\t\t\t\tstopStartupWatchdog();\n\t\t\t\tlogger.endTiming();\n\t\t\t\tawait runRpcMode(session, {\n\t\t\t\t\tsetToolUIContext: mode === \"rpc-ui\" ? setToolUIContext : undefined,\n",
			replace: "\t\t\t\tconst runRpcMode: RunRpcMode = (await import(\"./modes/rpc/rpc-mode\")).runRpcMode;\n\t\t\t\t// Ait native host: greet the bridge before the official ready frame.\n\t\t\t\tconst nativeHostHello =\n\t\t\t\t\tmode === \"rpc-ui\"\n\t\t\t\t\t\t? (await import(\"./modes/rpc/ait-native-host-hello\")).AIT_NATIVE_HOST_HELLO\n\t\t\t\t\t\t: undefined;\n\t\t\t\tstopStartupWatchdog();\n\t\t\t\tlogger.endTiming();\n\t\t\t\tawait runRpcMode(session, {\n\t\t\t\t\tnativeHostHello,\n\t\t\t\t\tsetToolUIContext: mode === \"rpc-ui\" ? setToolUIContext : undefined,\n",
		},
	],
});

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

async function readOrUndefined(file) {
	try {
		return await readFile(file);
	} catch (error) {
		if (error.code === "ENOENT") return undefined;
		throw error;
	}
}

/**
 * Patch the installed official SDK at `sdkDirectory` with the Ait native host bridge.
 * Returns the applied copy and edit inventory; throws a specific error instead of skipping.
 */
export async function patchOfficialRuntime(sdkDirectory) {
	const sdk = path.resolve(sdkDirectory);
	let pkg;
	try {
		pkg = JSON.parse(await readFile(path.join(sdk, "package.json"), "utf8"));
	} catch (error) {
		throw new Error(`Cannot patch ${sdk}: official SDK package.json is unreadable (${error.message})`);
	}
	assert.equal(pkg.name, "@oh-my-pi/pi-coding-agent", `Expected @oh-my-pi/pi-coding-agent in ${sdk}`);
	assert.equal(pkg.version, SDK_VERSION, `Expected ${SDK_VERSION} in ${sdk}, found ${pkg.version}`);
	for (const file of ADAPTER_IMPORTS) {
		if (!(await readOrUndefined(path.join(sdk, file)))) {
			throw new Error(`Ait adapter import target is missing from the official SDK: ${file}`);
		}
	}
	const copied = [];
	for (const [source, target] of COPIES) {
		const bytes = await readFile(path.join(assets, source));
		const destination = path.join(sdk, target);
		await mkdir(path.dirname(destination), { recursive: true });
		const existing = await readOrUndefined(destination);
		if (existing?.equals(bytes)) {
			copied.push({ path: target, action: "unchanged" });
			continue;
		}
		await writeFile(destination, bytes);
		copied.push({ path: target, action: existing ? "replaced" : "created" });
	}
	const helloPath = path.join(sdk, HELLO_MODULE);
	const hello = await readOrUndefined(helloPath);
	if (hello?.toString("utf8") !== HELLO_SOURCE) {
		await writeFile(helloPath, HELLO_SOURCE);
		copied.push({ path: HELLO_MODULE, action: hello ? "replaced" : "created" });
	} else {
		copied.push({ path: HELLO_MODULE, action: "unchanged" });
	}
	const edited = [];
	for (const file of Object.keys(EDITS)) edited.push(await patchFile(sdk, file));
	return { sdk, copied, edited };
}

async function patchFile(sdk, file) {
	const target = path.join(sdk, file);
	const original = await readOrUndefined(target);
	if (!original) throw new Error(`Ait native host patch target is missing: ${file}`);
	const before = original.toString("utf8");
	const digest = sha256(original);
	const edits = EDITS[file];
	const alreadyPatched = edits.every(({ replace }) => before.includes(replace));
	if (alreadyPatched) return { path: file, action: "unchanged" };
	if (digest !== EXPECTED_SHA256[file]) {
		const partial = edits.some(({ replace }) => before.includes(replace));
		throw new Error(
			`Ait native host patch refused: ${file} is not the expected ${SDK_VERSION} source ` +
				`(sha256 ${digest}, expected ${EXPECTED_SHA256[file]}${partial ? ", partially patched" : ""})`,
		);
	}
	let text = before;
	for (const { find, replace } of edits) {
		if (text.includes(replace)) continue;
		const index = text.indexOf(find);
		if (index < 0) throw new Error(`Ait native host patch anchor not found in ${file}: ${JSON.stringify(find)}`);
		if (text.indexOf(find, index + 1) >= 0) {
			throw new Error(`Ait native host patch anchor is ambiguous in ${file}: ${JSON.stringify(find)}`);
		}
		text = `${text.slice(0, index)}${replace}${text.slice(index + find.length)}`;
	}
	for (const { replace } of edits) {
		if (!text.includes(replace)) throw new Error(`Ait native host patch did not apply to ${file}`);
	}
	await writeFile(target, text);
	return { path: file, action: "patched" };
}
