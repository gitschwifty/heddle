import { debug } from "../debug.ts";
import { matchesHook } from "./matcher.ts";
import type { HookContext, HookEvent, HookResult, ResolvedHookDefinition, ResolvedHooksConfig } from "./types.ts";

export class HooksRunner {
	private config: ResolvedHooksConfig;
	private mode: "interactive" | "headless";
	private baseContext: { sessionId: string; project: string; model: string };

	constructor(
		config: ResolvedHooksConfig,
		mode: "interactive" | "headless",
		baseContext: { sessionId: string; project: string; model: string },
	) {
		this.config = config;
		this.mode = mode;
		this.baseContext = baseContext;
	}

	async run(event: HookEvent, context: Partial<HookContext>): Promise<HookResult[]> {
		const hooks = this.config[event];
		if (!hooks || hooks.length === 0) return [];

		const fullContext: HookContext = {
			sessionId: this.baseContext.sessionId,
			project: this.baseContext.project,
			model: this.baseContext.model,
			event,
			...context,
		};

		// Filter by mode
		const modeFiltered = hooks.filter((h) => h.mode === this.mode || h.mode === "both");

		// Filter by matchers
		const matched = modeFiltered.filter((h) => matchesHook(h, fullContext));

		// Split sync and async
		const syncHooks = matched.filter((h) => !h.async);
		const asyncHooks = matched.filter((h) => h.async);

		// Fire async hooks (fire-and-forget)
		for (const hook of asyncHooks) {
			this.executeAsync(hook, fullContext);
		}

		// Execute sync hooks sequentially
		const results: HookResult[] = [];
		for (const hook of syncHooks) {
			const result = await this.executeSync(hook, fullContext);
			results.push(result);
		}

		return results;
	}

	private async executeSync(hook: ResolvedHookDefinition, context: HookContext): Promise<HookResult> {
		const env = this.buildEnv(context);
		const stdinData = this.buildStdin(context);

		try {
			const proc = Bun.spawn(["sh", "-c", hook.command], {
				env: { ...process.env, ...env },
				stdin: "pipe",
				stdout: "pipe",
				stderr: "pipe",
			});

			// Write stdin data and close
			if (stdinData) {
				proc.stdin.write(stdinData);
			}
			proc.stdin.end();

			// Race between process completion and timeout
			const timeoutPromise = new Promise<"timeout">((resolve) => {
				setTimeout(() => resolve("timeout"), hook.timeout);
			});

			const exitPromise = proc.exited.then((code) => ({ code }));
			const raceResult = await Promise.race([exitPromise, timeoutPromise]);

			if (raceResult === "timeout") {
				proc.kill();
				return { blocked: false, timedOut: true };
			}

			const { code } = raceResult;
			const stdout = await new Response(proc.stdout).text();
			const stderr = await new Response(proc.stderr).text();

			if (code !== 0) {
				return {
					blocked: true,
					reason: stderr.trim() || `Hook exited with code ${code}`,
					timedOut: false,
				};
			}

			return {
				blocked: false,
				feedback: stdout.trim() || undefined,
				timedOut: false,
			};
		} catch (err) {
			debug("hooks", `Hook execution error: ${err}`);
			return { blocked: false, timedOut: false };
		}
	}

	private executeAsync(hook: ResolvedHookDefinition, context: HookContext): void {
		const env = this.buildEnv(context);
		const stdinData = this.buildStdin(context);

		try {
			const proc = Bun.spawn(["sh", "-c", hook.command], {
				env: { ...process.env, ...env },
				stdin: "pipe",
				stdout: "ignore",
				stderr: "pipe",
			});

			if (stdinData) {
				proc.stdin.write(stdinData);
			}
			proc.stdin.end();

			// Set up timeout to kill the process
			const timer = setTimeout(() => {
				proc.kill();
			}, hook.timeout);

			// Log warnings on failure (but don't block)
			proc.exited.then((code) => {
				clearTimeout(timer);
				if (code !== 0) {
					new Response(proc.stderr).text().then((stderr) => {
						debug("hooks", `Async hook warning (exit ${code}): ${stderr.trim()}`);
					});
				}
			});
		} catch (err) {
			debug("hooks", `Async hook spawn error: ${err}`);
		}
	}

	private buildEnv(context: HookContext): Record<string, string> {
		const env: Record<string, string> = {
			HEDDLE_HOOK_EVENT: context.event,
			HEDDLE_HOOK_SESSION_ID: context.sessionId,
			HEDDLE_HOOK_PROJECT: context.project,
			HEDDLE_HOOK_MODEL: context.model,
		};
		if (context.toolName) {
			env.HEDDLE_HOOK_TOOL_NAME = context.toolName;
		}
		return env;
	}

	private buildStdin(context: HookContext): string | null {
		const data: Record<string, string | undefined> = {};
		let hasData = false;

		if (context.toolArgs !== undefined) {
			data.tool_args = context.toolArgs;
			hasData = true;
		}
		if (context.toolResult !== undefined) {
			data.tool_result = context.toolResult;
			hasData = true;
		}
		if (context.userInput !== undefined) {
			data.user_input = context.userInput;
			hasData = true;
		}

		return hasData ? JSON.stringify(data) : null;
	}
}
