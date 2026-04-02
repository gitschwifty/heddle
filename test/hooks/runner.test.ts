import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { HooksRunner } from "../../src/hooks/runner.ts";
import type { ResolvedHooksConfig } from "../../src/hooks/types.ts";
import { createTestSandbox } from "../helpers/sandbox.ts";

let sandbox: ReturnType<typeof createTestSandbox>;
let scriptDir: string;

beforeAll(() => {
	sandbox = createTestSandbox("hooks-runner");
	scriptDir = join(sandbox.root, "scripts");
	mkdirSync(scriptDir, { recursive: true });
});

afterAll(() => {
	sandbox.cleanup();
});

const baseCtx = {
	sessionId: "sess-123",
	project: "/test/project",
	model: "test-model",
};

function makeScript(name: string, content: string): string {
	const path = join(scriptDir, name);
	writeFileSync(path, content, { mode: 0o755 });
	return path;
}

describe("HooksRunner", () => {
	test("successful hook — exit 0, stdout as feedback", async () => {
		const script = makeScript("ok.sh", '#!/bin/sh\necho "all good"');
		const config: ResolvedHooksConfig = {
			pre_tool: [{ command: script, timeout: 5000, mode: "both", async: false }],
		};
		const runner = new HooksRunner(config, "interactive", baseCtx);
		const results = await runner.run("pre_tool", {});
		expect(results).toHaveLength(1);
		expect(results[0]!.blocked).toBe(false);
		expect(results[0]!.feedback).toBe("all good");
		expect(results[0]!.timedOut).toBe(false);
	});

	test("blocking hook — non-zero exit, stderr as reason", async () => {
		const script = makeScript("block.sh", '#!/bin/sh\necho "forbidden" >&2\nexit 1');
		const config: ResolvedHooksConfig = {
			pre_tool: [{ command: script, timeout: 5000, mode: "both", async: false }],
		};
		const runner = new HooksRunner(config, "interactive", baseCtx);
		const results = await runner.run("pre_tool", {});
		expect(results).toHaveLength(1);
		expect(results[0]!.blocked).toBe(true);
		expect(results[0]!.reason).toBe("forbidden");
		expect(results[0]!.timedOut).toBe(false);
	});

	test("timeout handling — sync hook killed and timedOut set", async () => {
		const script = makeScript("slow.sh", "#!/bin/sh\nsleep 30");
		const config: ResolvedHooksConfig = {
			pre_tool: [{ command: script, timeout: 200, mode: "both", async: false }],
		};
		const runner = new HooksRunner(config, "interactive", baseCtx);
		const results = await runner.run("pre_tool", {});
		expect(results).toHaveLength(1);
		expect(results[0]!.timedOut).toBe(true);
		expect(results[0]!.blocked).toBe(false);
	});

	test("mode filtering — hook with mode=interactive skipped in headless", async () => {
		const script = makeScript("interactive-only.sh", '#!/bin/sh\necho "interactive"');
		const config: ResolvedHooksConfig = {
			pre_tool: [{ command: script, timeout: 5000, mode: "interactive", async: false }],
		};
		const runner = new HooksRunner(config, "headless", baseCtx);
		const results = await runner.run("pre_tool", {});
		expect(results).toHaveLength(0);
	});

	test("mode filtering — hook with mode=both runs in both modes", async () => {
		const script = makeScript("both-mode.sh", '#!/bin/sh\necho "both"');
		const config: ResolvedHooksConfig = {
			pre_tool: [{ command: script, timeout: 5000, mode: "both", async: false }],
		};
		const interactiveRunner = new HooksRunner(config, "interactive", baseCtx);
		const headlessRunner = new HooksRunner(config, "headless", baseCtx);
		expect(await interactiveRunner.run("pre_tool", {})).toHaveLength(1);
		expect(await headlessRunner.run("pre_tool", {})).toHaveLength(1);
	});

	test("env vars are set correctly", async () => {
		const script = makeScript(
			"env.sh",
			'#!/bin/sh\necho "$HEDDLE_HOOK_EVENT|$HEDDLE_HOOK_SESSION_ID|$HEDDLE_HOOK_PROJECT|$HEDDLE_HOOK_MODEL|$HEDDLE_HOOK_TOOL_NAME"',
		);
		const config: ResolvedHooksConfig = {
			pre_tool: [{ command: script, timeout: 5000, mode: "both", async: false }],
		};
		const runner = new HooksRunner(config, "interactive", baseCtx);
		const results = await runner.run("pre_tool", { toolName: "read" });
		expect(results[0]!.feedback).toBe("pre_tool|sess-123|/test/project|test-model|read");
	});

	test("stdin piping — large data sent via stdin as JSON", async () => {
		const script = makeScript("stdin.sh", "#!/bin/sh\ncat");
		const config: ResolvedHooksConfig = {
			pre_tool: [{ command: script, timeout: 5000, mode: "both", async: false }],
		};
		const runner = new HooksRunner(config, "interactive", baseCtx);
		const results = await runner.run("pre_tool", {
			toolArgs: '{"file_path":"test.ts"}',
			toolResult: "file contents here",
			userInput: "read the file",
		});
		const parsed = JSON.parse(results[0]!.feedback!);
		expect(parsed.tool_args).toBe('{"file_path":"test.ts"}');
		expect(parsed.tool_result).toBe("file contents here");
		expect(parsed.user_input).toBe("read the file");
	});

	test("async fire-and-forget — returns immediately, cannot block", async () => {
		// Writes a marker file to prove it ran, but we don't await it
		const marker = join(scriptDir, "async-marker.txt");
		const script = makeScript("async.sh", `#!/bin/sh\nsleep 0.1\necho "ran" > "${marker}"`);
		const config: ResolvedHooksConfig = {
			pre_tool: [{ command: script, timeout: 5000, mode: "both", async: true }],
		};
		const runner = new HooksRunner(config, "interactive", baseCtx);
		const start = performance.now();
		const results = await runner.run("pre_tool", {});
		const elapsed = performance.now() - start;
		// Async hooks don't return results
		expect(results).toHaveLength(0);
		// Should return quickly (not wait 100ms for script)
		expect(elapsed).toBeLessThan(100);
	});

	test("async hooks with non-zero exit don't block", async () => {
		const script = makeScript("async-fail.sh", '#!/bin/sh\necho "error" >&2\nexit 1');
		const config: ResolvedHooksConfig = {
			pre_tool: [{ command: script, timeout: 5000, mode: "both", async: true }],
		};
		const runner = new HooksRunner(config, "interactive", baseCtx);
		const results = await runner.run("pre_tool", {});
		// No results from async hooks
		expect(results).toHaveLength(0);
	});

	test("no hooks for event returns empty results", async () => {
		const config: ResolvedHooksConfig = {};
		const runner = new HooksRunner(config, "interactive", baseCtx);
		const results = await runner.run("pre_tool", {});
		expect(results).toHaveLength(0);
	});

	test("multiple sync hooks run sequentially", async () => {
		const script1 = makeScript("seq1.sh", '#!/bin/sh\necho "first"');
		const script2 = makeScript("seq2.sh", '#!/bin/sh\necho "second"');
		const config: ResolvedHooksConfig = {
			pre_tool: [
				{ command: script1, timeout: 5000, mode: "both", async: false },
				{ command: script2, timeout: 5000, mode: "both", async: false },
			],
		};
		const runner = new HooksRunner(config, "interactive", baseCtx);
		const results = await runner.run("pre_tool", {});
		expect(results).toHaveLength(2);
		expect(results[0]!.feedback).toBe("first");
		expect(results[1]!.feedback).toBe("second");
	});
});
