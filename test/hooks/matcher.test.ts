import { describe, expect, test } from "bun:test";
import { matchesHook } from "../../src/hooks/matcher.ts";
import type { HookContext, ResolvedHookDefinition } from "../../src/hooks/types.ts";

const baseContext: HookContext = {
	sessionId: "test-session",
	project: "/test/project",
	model: "test-model",
	event: "pre_tool",
};

function hook(overrides: Partial<ResolvedHookDefinition> = {}): ResolvedHookDefinition {
	return {
		command: "echo test",
		timeout: 10000,
		mode: "both",
		async: false,
		...overrides,
	};
}

describe("matchesHook", () => {
	test("no matchers matches everything", () => {
		expect(matchesHook(hook(), baseContext)).toBe(true);
		expect(matchesHook(hook(), { ...baseContext, toolName: "read" })).toBe(true);
	});

	test("tool matcher — exact string match", () => {
		const h = hook({ matchers: { tool: "read" } });
		expect(matchesHook(h, { ...baseContext, toolName: "read" })).toBe(true);
		expect(matchesHook(h, { ...baseContext, toolName: "write" })).toBe(false);
	});

	test("tool matcher — array inclusion", () => {
		const h = hook({ matchers: { tool: ["read", "write"] } });
		expect(matchesHook(h, { ...baseContext, toolName: "read" })).toBe(true);
		expect(matchesHook(h, { ...baseContext, toolName: "write" })).toBe(true);
		expect(matchesHook(h, { ...baseContext, toolName: "bash" })).toBe(false);
	});

	test("tool matcher — no toolName in context fails", () => {
		const h = hook({ matchers: { tool: "read" } });
		expect(matchesHook(h, baseContext)).toBe(false);
	});

	test("match_path — glob against file_path in toolArgs", () => {
		const h = hook({ matchers: { match_path: "**/*.ts" } });
		const ctx = { ...baseContext, toolArgs: JSON.stringify({ file_path: "src/hooks/types.ts" }) };
		expect(matchesHook(h, ctx)).toBe(true);
	});

	test("match_path — non-matching path", () => {
		const h = hook({ matchers: { match_path: "**/*.rs" } });
		const ctx = { ...baseContext, toolArgs: JSON.stringify({ file_path: "src/hooks/types.ts" }) };
		expect(matchesHook(h, ctx)).toBe(false);
	});

	test("match_path — no file_path in toolArgs fails", () => {
		const h = hook({ matchers: { match_path: "**/*.ts" } });
		const ctx = { ...baseContext, toolArgs: JSON.stringify({ command: "ls" }) };
		expect(matchesHook(h, ctx)).toBe(false);
	});

	test("match_path — no toolArgs fails", () => {
		const h = hook({ matchers: { match_path: "**/*.ts" } });
		expect(matchesHook(h, baseContext)).toBe(false);
	});

	test("match_args — glob against entire toolArgs string", () => {
		const h = hook({ matchers: { match_args: "*secret*" } });
		const ctx = { ...baseContext, toolArgs: '{"command":"cat secret.txt"}' };
		expect(matchesHook(h, ctx)).toBe(true);
	});

	test("match_args — non-matching args", () => {
		const h = hook({ matchers: { match_args: "*secret*" } });
		const ctx = { ...baseContext, toolArgs: '{"command":"cat readme.md"}' };
		expect(matchesHook(h, ctx)).toBe(false);
	});

	test("match_input — glob against userInput", () => {
		const h = hook({ matchers: { match_input: "*deploy*" } });
		const ctx = { ...baseContext, userInput: "please deploy to production" };
		expect(matchesHook(h, ctx)).toBe(true);
	});

	test("match_input — non-matching input", () => {
		const h = hook({ matchers: { match_input: "*deploy*" } });
		const ctx = { ...baseContext, userInput: "write some tests" };
		expect(matchesHook(h, ctx)).toBe(false);
	});

	test("match_input — no userInput fails", () => {
		const h = hook({ matchers: { match_input: "*deploy*" } });
		expect(matchesHook(h, baseContext)).toBe(false);
	});

	test("combined matchers — AND logic (all must pass)", () => {
		const h = hook({
			matchers: {
				tool: "write",
				match_path: "**/*.ts",
			},
		});
		// Both match
		const ctx1 = {
			...baseContext,
			toolName: "write",
			toolArgs: JSON.stringify({ file_path: "src/index.ts" }),
		};
		expect(matchesHook(h, ctx1)).toBe(true);

		// Tool matches but path doesn't
		const ctx2 = {
			...baseContext,
			toolName: "write",
			toolArgs: JSON.stringify({ file_path: "src/index.rs" }),
		};
		expect(matchesHook(h, ctx2)).toBe(false);

		// Path matches but tool doesn't
		const ctx3 = {
			...baseContext,
			toolName: "read",
			toolArgs: JSON.stringify({ file_path: "src/index.ts" }),
		};
		expect(matchesHook(h, ctx3)).toBe(false);
	});
});
