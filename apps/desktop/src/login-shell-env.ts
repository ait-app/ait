// Shell environment resolution adapted from VS Code
// https://github.com/microsoft/vscode/blob/main/src/vs/platform/shell/node/shellEnv.ts
// Licensed under the MIT License.

import type { SpawnSyncReturns } from "node:child_process";
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { userInfo as defaultUserInfo } from "node:os";
import { basename } from "node:path";
import defaultLog from "electron-log/main";

const DEFAULT_RESOLVE_TIMEOUT_MS = 30_000;
const TIMEOUT_ENV_KEY = "AIT_SHELL_ENV_TIMEOUT_MS";
const STDERR_LOG_LIMIT = 2000;
/** A shell that ignores SIGTERM after a timeout is force-killed after this grace period. */
const KILL_GRACE_MS = 1000;
/** Background jobs started by shell startup files may hold stdout open after the shell exits. */
const EXIT_DRAIN_MS = 250;

type LoginShellEnvLogger = Pick<typeof defaultLog, "info" | "warn">;
type ShellEnvAttemptKind = "interactive" | "non-interactive";

export interface ShellRunOptions {
  argv0?: string;
  timeout: number;
  env: NodeJS.ProcessEnv;
}

/** Run a shell and report its outcome with `spawnSync` result semantics. */
export type ShellRunner = (
  shell: string,
  args: string[],
  options: ShellRunOptions,
) => Promise<SpawnSyncReturns<string>>;

interface LoginShellEnvDependencies {
  env?: NodeJS.ProcessEnv;
  logger?: LoginShellEnvLogger;
  now?: () => number;
  platform?: NodeJS.Platform;
  runShell?: ShellRunner;
  userInfo?: typeof defaultUserInfo;
}

function truncateForLog(value: string | undefined): string | undefined {
  if (!value) return undefined;
  const trimmed = value.trim();
  if (!trimmed) return undefined;
  return trimmed.length > STDERR_LOG_LIMIT
    ? `${trimmed.slice(0, STDERR_LOG_LIMIT)}...(truncated)`
    : trimmed;
}

function pathEnv(env: NodeJS.ProcessEnv | Record<string, string>): string | null {
  return env.PATH ?? env.Path ?? null;
}

interface ShellEnvErrorDetails {
  reason: string;
  attemptKind?: ShellEnvAttemptKind;
  argv0?: string;
  shell?: string;
  shellArgs?: string[];
  status?: number | null;
  signal?: NodeJS.Signals | null;
  stdoutLength?: number;
  markerFound?: boolean;
  stderr?: string;
}

class ShellEnvError extends Error {
  constructor(
    message: string,
    readonly details: ShellEnvErrorDetails,
    options?: { cause?: unknown },
  ) {
    super(message, options);
    this.name = "ShellEnvError";
  }
}

interface ShellEnvAttempt {
  kind: ShellEnvAttemptKind;
  argv0?: string;
  shellArgs: string[];
}

interface ShellEnvCommand {
  command: string;
  attempts: ShellEnvAttempt[];
}

interface ResolvedShellEnv {
  env: Record<string, string>;
  attemptKind: ShellEnvAttemptKind;
}

interface AttemptTimeoutInput {
  totalTimeoutMs: number;
  attemptsStartedAt: number;
  now: () => number;
  attempts: ShellEnvAttempt[];
  index: number;
}

interface ThrowIfShellFailedInput {
  result: SpawnSyncReturns<string>;
  regex: RegExp;
  shell: string;
  attempt: ShellEnvAttempt;
}

interface ShellEnvForAttemptInput {
  deps: Required<LoginShellEnvDependencies>;
  shellEnv: NodeJS.ProcessEnv;
  shell: string;
  command: string;
  regex: RegExp;
  attempt: ShellEnvAttempt;
  timeoutMs: number;
}

interface ShellAttemptErrorDetailsInput {
  error: unknown;
  shell: string;
  attempt: ShellEnvAttempt;
}

interface LogShellAttemptFailureInput {
  deps: Required<LoginShellEnvDependencies>;
  error: unknown;
  details: ShellEnvErrorDetails;
  durationMs: number;
  timeoutMs: number;
  willRetry: boolean;
}

interface RestoreElectronEnvInput {
  env: Record<string, string>;
  savedRunAsNode: string | undefined;
  savedNoAttach: string | undefined;
}

interface ResolveShellEnvInput {
  deps: Required<LoginShellEnvDependencies>;
  timeoutMs: number;
}

function timeoutMsFromEnv(env: NodeJS.ProcessEnv): number {
  const rawTimeoutMs = env[TIMEOUT_ENV_KEY];
  if (!rawTimeoutMs) return DEFAULT_RESOLVE_TIMEOUT_MS;

  const timeoutMs = Number.parseInt(rawTimeoutMs, 10);
  return Number.isFinite(timeoutMs) && timeoutMs > 0 ? timeoutMs : DEFAULT_RESOLVE_TIMEOUT_MS;
}

function timeoutMsForAttempt({
  totalTimeoutMs,
  attemptsStartedAt,
  now,
  attempts,
  index,
}: AttemptTimeoutInput): number | null {
  if (attempts.length === 1) return totalTimeoutMs;
  if (index === 0) return Math.max(1, Math.floor(totalTimeoutMs / 2));

  const remainingMs = totalTimeoutMs - (now() - attemptsStartedAt);
  return remainingMs > 0 ? remainingMs : null;
}

function errorCode(error: unknown): string | null {
  return error instanceof Error ? ((error as NodeJS.ErrnoException).code ?? null) : null;
}

function shellFailureReason(result: SpawnSyncReturns<string>): string {
  if (errorCode(result.error) === "ETIMEDOUT") return "timeout";
  return result.error ? "spawn-error" : "signal";
}

function attemptFailureDetails(
  reason: string,
  shell: string,
  attempt: ShellEnvAttempt,
  result: SpawnSyncReturns<string>,
  markerFound: boolean,
): ShellEnvErrorDetails {
  return {
    reason,
    attemptKind: attempt.kind,
    argv0: attempt.argv0,
    shell,
    shellArgs: attempt.shellArgs,
    status: result.status,
    signal: result.signal,
    stdoutLength: result.stdout?.length ?? 0,
    markerFound,
    stderr: result.stderr,
  };
}

function throwIfShellFailed({ result, regex, shell, attempt }: ThrowIfShellFailedInput): void {
  if (result.error || result.signal) {
    throw new ShellEnvError(
      "login shell did not complete",
      attemptFailureDetails(
        shellFailureReason(result),
        shell,
        attempt,
        result,
        regex.test(result.stdout ?? ""),
      ),
      { cause: result.error },
    );
  }
  if (result.status !== 0 && result.status !== null) {
    throw new ShellEnvError(
      "login shell exited non-zero",
      attemptFailureDetails(
        "non-zero-exit",
        shell,
        attempt,
        result,
        regex.test(result.stdout ?? ""),
      ),
    );
  }
  if (!result.stdout) {
    throw new ShellEnvError(
      "login shell produced no stdout",
      attemptFailureDetails("no-stdout", shell, attempt, result, false),
      { cause: result.error },
    );
  }
}

/**
 * Asynchronous counterpart of `spawnSync` for the login shell: collects UTF-8 output and,
 * after `timeout`, kills the shell and reports an `ETIMEDOUT` error like `spawnSync` does.
 */
export const runShell: ShellRunner = (shell, args, options) =>
  new Promise((resolve) => {
    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    let error: Error | undefined;
    let settled = false;
    let timeout: NodeJS.Timeout | undefined;
    let forceKill: NodeJS.Timeout | undefined;
    let drain: NodeJS.Timeout | undefined;
    const child = spawn(shell, args, {
      argv0: options.argv0,
      env: options.env,
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    });
    const settle = (status: number | null, signal: NodeJS.Signals | null) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      clearTimeout(forceKill);
      clearTimeout(drain);
      const out = Buffer.concat(stdout).toString("utf8");
      const err = Buffer.concat(stderr).toString("utf8");
      resolve({
        pid: child.pid ?? 0,
        output: [null, out, err],
        stdout: out,
        stderr: err,
        status,
        signal,
        error,
      });
    };
    timeout = setTimeout(() => {
      error = Object.assign(new Error(`spawn ${shell} ETIMEDOUT`), { code: "ETIMEDOUT" });
      child.kill("SIGTERM");
      forceKill = setTimeout(() => {
        child.kill("SIGKILL");
        settle(null, "SIGKILL");
      }, KILL_GRACE_MS);
    }, options.timeout);
    child.stdout.on("data", (chunk: Buffer) => stdout.push(chunk));
    child.stderr.on("data", (chunk: Buffer) => stderr.push(chunk));
    child.once("error", (spawnError) => {
      error ??= spawnError;
      if (child.pid === undefined) settle(null, null);
    });
    child.once("exit", (status, signal) => {
      drain = setTimeout(() => settle(status, signal), EXIT_DRAIN_MS);
    });
    child.once("close", (status, signal) => settle(status, signal));
  });

function getSystemShell(
  deps: Required<Pick<LoginShellEnvDependencies, "env" | "platform" | "userInfo">>,
): string {
  const shell = deps.env.SHELL;
  if (shell) return shell;

  try {
    const info = deps.userInfo();
    if (info.shell && info.shell !== "/bin/false") return info.shell;
  } catch {}

  return deps.platform === "darwin" ? "/bin/zsh" : "/bin/bash";
}

function shellEnvCommand({ shell, mark }: { shell: string; mark: string }): ShellEnvCommand {
  const name = basename(shell);

  if (/^(?:pwsh|powershell)(?:-preview)?$/.test(name)) {
    return {
      command: `& '${process.execPath}' -p '''${mark}'' + JSON.stringify(process.env) + ''${mark}'''`,
      attempts: [{ kind: "non-interactive", shellArgs: ["-Login", "-Command"] }],
    };
  }

  if (name === "nu") {
    return {
      command: `^'${process.execPath}' -p '"${mark}" + JSON.stringify(process.env) + "${mark}"'`,
      attempts: [
        { kind: "interactive", shellArgs: ["-i", "-l", "-c"] },
        { kind: "non-interactive", shellArgs: ["-l", "-c"] },
      ],
    };
  }

  if (name === "xonsh") {
    return {
      command: `import os, json; print("${mark}", json.dumps(dict(os.environ)), "${mark}")`,
      attempts: [
        { kind: "interactive", shellArgs: ["-i", "-l", "-c"] },
        { kind: "non-interactive", shellArgs: ["-l", "-c"] },
      ],
    };
  }

  if (name === "tcsh" || name === "csh") {
    return {
      command: `'${process.execPath}' -p '"${mark}" + JSON.stringify(process.env) + "${mark}"'`,
      attempts: [
        { kind: "interactive", shellArgs: ["-ic"] },
        { kind: "non-interactive", argv0: `-${name}`, shellArgs: ["-c"] },
      ],
    };
  }

  return {
    command: `'${process.execPath}' -p '"${mark}" + JSON.stringify(process.env) + "${mark}"'`,
    attempts: [
      { kind: "interactive", shellArgs: ["-i", "-l", "-c"] },
      { kind: "non-interactive", shellArgs: ["-l", "-c"] },
    ],
  };
}

async function shellEnvForAttempt({
  deps,
  shellEnv,
  shell,
  command,
  regex,
  attempt,
  timeoutMs,
}: ShellEnvForAttemptInput): Promise<Record<string, string>> {
  const result = await deps.runShell(shell, [...attempt.shellArgs, command], {
    argv0: attempt.argv0,
    timeout: timeoutMs,
    env: {
      ...shellEnv,
      ELECTRON_RUN_AS_NODE: "1",
      ELECTRON_NO_ATTACH_CONSOLE: "1",
    },
  });

  throwIfShellFailed({ result, regex, shell, attempt });

  const match = regex.exec(result.stdout);
  if (!match?.[1]) {
    throw new ShellEnvError(
      "login shell output did not contain environment marker",
      attemptFailureDetails("marker-missing", shell, attempt, result, false),
    );
  }

  try {
    return JSON.parse(match[1]) as Record<string, string>;
  } catch (error) {
    throw new ShellEnvError(
      "failed to parse login shell environment JSON",
      attemptFailureDetails("json-parse", shell, attempt, result, true),
      { cause: error },
    );
  }
}

function shellAttemptErrorDetails({
  error,
  shell,
  attempt,
}: ShellAttemptErrorDetailsInput): ShellEnvErrorDetails {
  return error instanceof ShellEnvError
    ? error.details
    : {
        reason: "throw",
        attemptKind: attempt.kind,
        argv0: attempt.argv0,
        shell,
        shellArgs: attempt.shellArgs,
      };
}

function logShellAttemptFailure({
  deps,
  error,
  details,
  durationMs,
  timeoutMs,
  willRetry,
}: LogShellAttemptFailureInput): void {
  const cause = error instanceof Error ? error.cause : undefined;
  deps.logger.warn(
    willRetry ? "[login-shell-env] attempt failed; retrying" : "[login-shell-env] attempt failed",
    {
      ...details,
      durationMs,
      timeoutMs,
      error: error instanceof Error ? error.message : String(error),
      errorCode: error instanceof ShellEnvError ? errorCode(cause) : errorCode(error),
      stderr: truncateForLog(details.stderr),
    },
  );
}

function restoreElectronEnv({ env, savedRunAsNode, savedNoAttach }: RestoreElectronEnvInput): void {
  if (savedRunAsNode) {
    env.ELECTRON_RUN_AS_NODE = savedRunAsNode;
  } else {
    delete env.ELECTRON_RUN_AS_NODE;
  }

  if (savedNoAttach) {
    env.ELECTRON_NO_ATTACH_CONSOLE = savedNoAttach;
  } else {
    delete env.ELECTRON_NO_ATTACH_CONSOLE;
  }

  delete env.XDG_RUNTIME_DIR;
}

async function resolveShellEnv({
  deps,
  timeoutMs,
}: ResolveShellEnvInput): Promise<ResolvedShellEnv> {
  if (deps.platform === "win32") {
    throw new ShellEnvError("login shell env is not resolved on Windows", { reason: "win32" });
  }

  const savedRunAsNode = deps.env.ELECTRON_RUN_AS_NODE;
  const savedNoAttach = deps.env.ELECTRON_NO_ATTACH_CONSOLE;

  const mark = randomUUID().replace(/-/g, "").slice(0, 12);
  const regex = new RegExp(mark + "({.*})" + mark);

  const shell = getSystemShell(deps);
  const { command, attempts } = shellEnvCommand({ shell, mark });

  const shellEnv = { ...deps.env };
  delete shellEnv.PASEO_NODE_ENV;
  delete shellEnv.PASEO_DESKTOP_MANAGED;
  delete shellEnv.PASEO_SUPERVISED;

  deps.logger.info("[login-shell-env] start", {
    shell,
    shellArgs: attempts[0]?.shellArgs ?? [],
    attempts: attempts.map((attempt) => ({
      attemptKind: attempt.kind,
      argv0: attempt.argv0,
      shellArgs: attempt.shellArgs,
    })),
    timeoutMs,
    beforePath: pathEnv(deps.env),
  });

  let lastError: unknown;
  const attemptsStartedAt = deps.now();

  for (const [index, attempt] of attempts.entries()) {
    const attemptTimeoutMs = timeoutMsForAttempt({
      totalTimeoutMs: timeoutMs,
      attemptsStartedAt,
      now: deps.now,
      attempts,
      index,
    });
    if (attemptTimeoutMs === null) break;

    const attemptStartedAt = deps.now();

    try {
      const env = await shellEnvForAttempt({
        deps,
        shellEnv,
        shell,
        command,
        regex,
        attempt,
        timeoutMs: attemptTimeoutMs,
      });
      const durationMs = deps.now() - attemptStartedAt;
      restoreElectronEnv({ env, savedRunAsNode, savedNoAttach });

      deps.logger.info("[login-shell-env] attempt applied", {
        attemptKind: attempt.kind,
        argv0: attempt.argv0,
        shell,
        shellArgs: attempt.shellArgs,
        reason: "success",
        durationMs,
        timeoutMs: attemptTimeoutMs,
      });

      return { env, attemptKind: attempt.kind };
    } catch (error) {
      const details = shellAttemptErrorDetails({ error, shell, attempt });
      const durationMs = deps.now() - attemptStartedAt;
      const willRetry =
        index < attempts.length - 1 &&
        timeoutMsForAttempt({
          totalTimeoutMs: timeoutMs,
          attemptsStartedAt,
          now: deps.now,
          attempts,
          index: index + 1,
        }) !== null;
      lastError = error;
      logShellAttemptFailure({
        deps,
        error,
        details,
        durationMs,
        timeoutMs: attemptTimeoutMs,
        willRetry,
      });
    }
  }

  throw lastError;
}

/**
 * On macOS/Linux, Electron inherits a minimal environment when launched from
 * Finder/Dock. Spawn the user's login shell and capture its full environment
 * via Node's JSON.stringify(process.env), so the daemon and all child processes
 * see the same tools and variables as a normal terminal session.
 *
 * The shell runs asynchronously so the window can load meanwhile; callers that spawn
 * processes or read the environment must wait for the returned promise. It never rejects:
 * when the lookup fails the inherited environment is kept.
 *
 * Approach borrowed from VS Code (src/vs/platform/shell/node/shellEnv.ts).
 */
export async function inheritLoginShellEnv(input: LoginShellEnvDependencies = {}): Promise<void> {
  const deps: Required<LoginShellEnvDependencies> = {
    env: input.env ?? process.env,
    logger: input.logger ?? defaultLog,
    now: input.now ?? Date.now,
    platform: input.platform ?? process.platform,
    runShell: input.runShell ?? runShell,
    userInfo: input.userInfo ?? defaultUserInfo,
  };
  const beforePath = pathEnv(deps.env);
  const startedAt = deps.now();
  const timeoutMs = timeoutMsFromEnv(deps.env);

  try {
    const { env, attemptKind } = await resolveShellEnv({ deps, timeoutMs });
    Object.assign(deps.env, env);
    deps.logger.info("[login-shell-env] applied", {
      attemptKind,
      durationMs: deps.now() - startedAt,
      timeoutMs,
      beforePath,
      afterPath: pathEnv(deps.env),
      pathChanged: beforePath !== pathEnv(deps.env),
      shell: deps.env.SHELL ?? null,
    });
  } catch (error) {
    const details: ShellEnvErrorDetails =
      error instanceof ShellEnvError
        ? error.details
        : { reason: "throw", shell: deps.env.SHELL ?? undefined };
    const cause = error instanceof Error ? error.cause : undefined;
    deps.logger.warn("[login-shell-env] failed; keeping inherited env", {
      ...details,
      durationMs: deps.now() - startedAt,
      timeoutMs,
      error: error instanceof Error ? error.message : String(error),
      errorCode: (cause as NodeJS.ErrnoException | undefined)?.code ?? null,
      stderr: truncateForLog(details.stderr),
      beforePath,
      afterPath: pathEnv(deps.env),
      pathChanged: beforePath !== pathEnv(deps.env),
    });
    // Keep inherited environment if shell lookup fails.
  }
}
