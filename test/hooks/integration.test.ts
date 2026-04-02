import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createTestSandbox } from "../helpers/sandbox.ts";
import { HooksRunner } from "../../src/hooks/runner.ts";
import type { ResolvedHooksConfig, HookContext } from "../../src/hooks/types.ts";

let sandbox: ReturnType<typeof createTestSandbox>;
let scriptDir: string;

beforeAll(() => {
	sandbox = createTestSandbox("hooks-integration");
	scriptDir = join(sandbox.root, "scripts");
	mkdirSync(scriptDir, { recursive: true });
});

afterAll(() => {
	sandbox.cleanup();
});

const baseCtx = {
	sessionId: "sess-int",
	project: "/test/project",
	model: "test-model",
};

function makeScript(name: string, content: string): string {
	const path = join(scriptDir, name);
	writeFileSync(path, content, { mode: 0o755 });
	return path;
}

describe("hooks integration", () => {
	test("pre_tool blocking skips execution — simulated flow", async () => {
		const script = makeScript("block-tool.sh", '#!/bin/sh\necho "blocked by policy" >&2\nexit 1');
		const config: ResolvedHooksConfig = {
			pre_tool: [{ command: script, timeout: 5000, mode: "both", async: false }],
		};
		const runner = new HooksRunner(config, "interactive", baseCtx);

		const context: Partial<HookContext> = {
			toolName: "bash",
			toolArgs: '{"command":"rm -rf /"}',
		};

		const results = await runner.run("pre_tool", context);
		expect(results.some((r) => r.blocked)).toBe(true);
		const blockedResult = results.find((r) => r.blocked);
		expect(blockedResult!.reason).toBe("blocked by policy");
	});

	test("post_tool feedback collected from stdout", async () => {
		const script = makeScript("post-feedback.sh", '#!/bin/sh\necho "[hook] file was written"');
		const config: ResolvedHooksConfig = {
			post_tool: [{ command: script, timeout: 5000, mode: "both", async: false }],
		};
		const runner = new HooksRunner(config, "interactive", baseCtx);

		const results = await runner.run("post_tool", {
			toolName: "write",
			toolResult: "File written successfully",
		});

		expect(results).toHaveLength(1);
		expect(results[0]!.feedback).toBe("[hook] file was written");
		expect(results[0]!.blocked).toBe(false);
	});

	test("pre_prompt blocking — hook rejects user input", async () => {
		const script = makeScript(
			"block-prompt.sh",
			'#!/bin/sh\n# Read stdin and check for blocked content\ninput=$(cat)\necho "$input" | grep -q "deploy" && { echo "deployments are disabled" >&2; exit 1; }\nexit 0',
		);
		const config: ResolvedHooksConfig = {
			pre_prompt: [{ command: script, timeout: 5000, mode: "both", async: false }],
		};
		const runner = new HooksRunner(config, "interactive", baseCtx);

		// Blocked input
		const blocked = await runner.run("pre_prompt", { userInput: "deploy to production" });
		expect(blocked.some((r) => r.blocked)).toBe(true);

		// Allowed input
		const allowed = await runner.run("pre_prompt", { userInput: "write some tests" });
		expect(allowed.every((r) => !r.blocked)).toBe(true);
	});

	test("mixed sync and async hooks in same event", async () => {
		const syncScript = makeScript("mixed-sync.sh", '#!/bin/sh\necho "sync feedback"');
		const asyncScript = makeScript("mixed-async.sh", "#!/bin/sh\nsleep 0.1");
		const config: ResolvedHooksConfig = {
			post_tool: [
				{ command: syncScript, timeout: 5000, mode: "both", async: false },
				{ command: asyncScript, timeout: 5000, mode: "both", async: true },
			],
		};
		const runner = new HooksRunner(config, "interactive", baseCtx);
		const results = await runner.run("post_tool", { toolName: "read" });
		// Only sync hook returns results
		expect(results).toHaveLength(1);
		expect(results[0]!.feedback).toBe("sync feedback");
	});

	test("matchers filter hooks before execution", async () => {
		const script = makeScript("matcher-filter.sh", '#!/bin/sh\necho "matched"');
		const config: ResolvedHooksConfig = {
			pre_tool: [
				{
					command: script,
					timeout: 5000,
					mode: "both",
					async: false,
					matchers: { tool: "write" },
				},
			],
		};
		const runner = new HooksRunner(config, "interactive", baseCtx);

		// Non-matching tool — hook should not run
		const readResults = await runner.run("pre_tool", { toolName: "read" });
		expect(readResults).toHaveLength(0);

		// Matching tool — hook should run
		const writeResults = await runner.run("pre_tool", { toolName: "write" });
		expect(writeResults).toHaveLength(1);
		expect(writeResults[0]!.feedback).toBe("matched");
	});
});
