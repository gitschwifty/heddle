import { describe, expect, test } from "bun:test";
import { PermissionChecker, readOnlyToolFilter } from "../../src/permissions/index.ts";
import type { ToolDefinition } from "../../src/types.ts";

describe("PermissionChecker", () => {
	// ── Mode: suggest ───────────────────────────────────────────────────
	describe("suggest mode", () => {
		const checker = new PermissionChecker("suggest");

		test("allows read tools", () => {
			expect(checker.check("read_file")).toEqual({ decision: "allow" });
			expect(checker.check("glob")).toEqual({ decision: "allow" });
			expect(checker.check("grep")).toEqual({ decision: "allow" });
		});

		test("asks for write tools", () => {
			const result = checker.check("write_file");
			expect(result.decision).toBe("ask");
		});

		test("asks for execute tools", () => {
			const result = checker.check("bash");
			expect(result.decision).toBe("ask");
		});

		test("allows network tools", () => {
			expect(checker.check("web_fetch")).toEqual({ decision: "allow" });
		});
	});

	// ── Mode: auto-edit ─────────────────────────────────────────────────
	describe("auto-edit mode", () => {
		const checker = new PermissionChecker("auto-edit");

		test("allows read tools", () => {
			expect(checker.check("read_file")).toEqual({ decision: "allow" });
		});

		test("allows write tools", () => {
			expect(checker.check("write_file")).toEqual({ decision: "allow" });
			expect(checker.check("edit_file")).toEqual({ decision: "allow" });
		});

		test("asks for execute tools", () => {
			const result = checker.check("bash");
			expect(result.decision).toBe("ask");
		});

		test("allows network tools", () => {
			expect(checker.check("web_fetch")).toEqual({ decision: "allow" });
		});
	});

	// ── Mode: full-auto ─────────────────────────────────────────────────
	describe("full-auto mode", () => {
		const checker = new PermissionChecker("full-auto");

		test("allows all tool categories", () => {
			expect(checker.check("read_file")).toEqual({ decision: "allow" });
			expect(checker.check("write_file")).toEqual({ decision: "allow" });
			expect(checker.check("bash")).toEqual({ decision: "allow" });
			expect(checker.check("web_fetch")).toEqual({ decision: "allow" });
		});
	});

	// ── Mode: plan ──────────────────────────────────────────────────────
	describe("plan mode", () => {
		const checker = new PermissionChecker("plan");

		test("allows read tools", () => {
			expect(checker.check("read_file")).toEqual({ decision: "allow" });
			expect(checker.check("glob")).toEqual({ decision: "allow" });
			expect(checker.check("grep")).toEqual({ decision: "allow" });
		});

		test("denies write tools", () => {
			const result = checker.check("write_file");
			expect(result.decision).toBe("deny");
			expect(result.reason).toBeDefined();
		});

		test("denies execute tools", () => {
			const result = checker.check("bash");
			expect(result.decision).toBe("deny");
			expect(result.reason).toBeDefined();
		});

		test("allows network tools", () => {
			expect(checker.check("web_fetch")).toEqual({ decision: "allow" });
		});
	});

	// ── Mode: yolo ──────────────────────────────────────────────────────
	describe("yolo mode", () => {
		const checker = new PermissionChecker("yolo");

		test("allows all tool categories (same as full-auto)", () => {
			expect(checker.check("read_file")).toEqual({ decision: "allow" });
			expect(checker.check("write_file")).toEqual({ decision: "allow" });
			expect(checker.check("bash")).toEqual({ decision: "allow" });
			expect(checker.check("web_fetch")).toEqual({ decision: "allow" });
		});
	});

	// ── Hardcoded protections ───────────────────────────────────────────
	describe("hardcoded .env protection", () => {
		const checker = new PermissionChecker("full-auto");

		test("denies writes to .env files", () => {
			const result = checker.check("write_file", { path: "/project/.env" });
			expect(result.decision).toBe("deny");
			expect(result.reason).toContain(".env");
		});

		test("denies writes to .env.local", () => {
			const result = checker.check("write_file", { path: "/project/.env.local" });
			expect(result.decision).toBe("deny");
			expect(result.reason).toContain(".env");
		});

		test("denies writes to .env.test", () => {
			const result = checker.check("edit_file", { path: ".env.test" });
			expect(result.decision).toBe("deny");
			expect(result.reason).toContain(".env");
		});

		test("allows writes to files that look like .env but are not", () => {
			const result = checker.check("write_file", { path: "/project/not-env-file.txt" });
			expect(result.decision).toBe("allow");
		});
	});

	describe("hardcoded rm protection", () => {
		const checker = new PermissionChecker("full-auto");

		test("denies rm commands", () => {
			const result = checker.check("bash", { command: "rm file.txt" });
			expect(result.decision).toBe("deny");
			expect(result.reason).toContain("rm");
		});

		test("denies rm -rf commands", () => {
			const result = checker.check("bash", { command: "rm -rf /tmp/stuff" });
			expect(result.decision).toBe("deny");
			expect(result.reason).toContain("rm");
		});

		test("allows non-rm bash commands", () => {
			const result = checker.check("bash", { command: "ls -la" });
			expect(result.decision).toBe("allow");
		});
	});

	// ── ask_user always allowed ─────────────────────────────────────────
	describe("ask_user tool", () => {
		test("always allowed in plan mode", () => {
			const checker = new PermissionChecker("plan");
			expect(checker.check("ask_user")).toEqual({ decision: "allow" });
		});

		test("always allowed in suggest mode", () => {
			const checker = new PermissionChecker("suggest");
			expect(checker.check("ask_user")).toEqual({ decision: "allow" });
		});
	});

	// ── web_fetch always allowed ────────────────────────────────────────
	describe("web_fetch tool", () => {
		test("always allowed in plan mode", () => {
			const checker = new PermissionChecker("plan");
			expect(checker.check("web_fetch")).toEqual({ decision: "allow" });
		});

		test("always allowed in suggest mode", () => {
			const checker = new PermissionChecker("suggest");
			expect(checker.check("web_fetch")).toEqual({ decision: "allow" });
		});
	});

	// ── Unknown tools ───────────────────────────────────────────────────
	describe("unknown tools", () => {
		test("default to execute category in suggest mode (ask)", () => {
			const checker = new PermissionChecker("suggest");
			const result = checker.check("unknown_future_tool");
			expect(result.decision).toBe("ask");
		});

		test("default to execute category in plan mode (deny)", () => {
			const checker = new PermissionChecker("plan");
			const result = checker.check("unknown_future_tool");
			expect(result.decision).toBe("deny");
		});
	});

	// ── allowAlways ─────────────────────────────────────────────────────
	describe("allowAlways", () => {
		test("bypasses ask after allowAlways called", () => {
			const checker = new PermissionChecker("suggest");
			// First check should ask
			expect(checker.check("bash").decision).toBe("ask");
			// After allowAlways, should allow
			checker.allowAlways("bash");
			expect(checker.check("bash")).toEqual({ decision: "allow" });
		});

		test("does not bypass hardcoded protections", () => {
			const checker = new PermissionChecker("suggest");
			checker.allowAlways("write_file");
			// Normal writes are allowed
			expect(checker.check("write_file", { path: "foo.txt" })).toEqual({ decision: "allow" });
			// .env writes are still denied
			const result = checker.check("write_file", { path: ".env" });
			expect(result.decision).toBe("deny");
		});
	});

	// ── Reasons ─────────────────────────────────────────────────────────
	describe("reasons", () => {
		test("deny returns a reason", () => {
			const checker = new PermissionChecker("plan");
			const result = checker.check("write_file");
			expect(result.decision).toBe("deny");
			expect(typeof result.reason).toBe("string");
			expect(result.reason!.length).toBeGreaterThan(0);
		});

		test("ask returns a reason", () => {
			const checker = new PermissionChecker("suggest");
			const result = checker.check("bash");
			expect(result.decision).toBe("ask");
			expect(typeof result.reason).toBe("string");
		});
	});
});

describe("readOnlyToolFilter", () => {
	const allTools: ToolDefinition[] = [
		{ type: "function", function: { name: "read_file", description: "Read", parameters: {} } },
		{ type: "function", function: { name: "glob", description: "Glob", parameters: {} } },
		{ type: "function", function: { name: "grep", description: "Grep", parameters: {} } },
		{ type: "function", function: { name: "ask_user", description: "Ask", parameters: {} } },
		{ type: "function", function: { name: "web_fetch", description: "Fetch", parameters: {} } },
		{ type: "function", function: { name: "write_file", description: "Write", parameters: {} } },
		{ type: "function", function: { name: "edit_file", description: "Edit", parameters: {} } },
		{ type: "function", function: { name: "bash", description: "Bash", parameters: {} } },
	];

	test("keeps read and network tools", () => {
		const filtered = readOnlyToolFilter(allTools);
		const names = filtered.map((t) => t.function.name);
		expect(names).toContain("read_file");
		expect(names).toContain("glob");
		expect(names).toContain("grep");
		expect(names).toContain("ask_user");
		expect(names).toContain("web_fetch");
	});

	test("removes write and execute tools", () => {
		const filtered = readOnlyToolFilter(allTools);
		const names = filtered.map((t) => t.function.name);
		expect(names).not.toContain("write_file");
		expect(names).not.toContain("edit_file");
		expect(names).not.toContain("bash");
	});
});
