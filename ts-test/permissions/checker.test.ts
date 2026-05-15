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
		test("allows all tool categories", () => {
			const checker = new PermissionChecker("yolo");
			expect(checker.check("read_file")).toEqual({ decision: "allow" });
			expect(checker.check("write_file")).toEqual({ decision: "allow" });
			expect(checker.check("bash")).toEqual({ decision: "allow" });
			expect(checker.check("web_fetch")).toEqual({ decision: "allow" });
		});

		test("ignores deny rules", () => {
			const checker = new PermissionChecker("yolo", {
				layers: [{ allow: [], deny: ["Write(.env*)"], ask: [] }],
			});
			const result = checker.check("write_file", { path: ".env" });
			expect(result.decision).toBe("allow");
		});

		test("ignores ask rules", () => {
			const checker = new PermissionChecker("yolo", {
				layers: [{ allow: [], deny: [], ask: ["Bash(git push *)"] }],
			});
			const result = checker.check("bash", { command: "git push origin main" });
			expect(result.decision).toBe("allow");
		});
	});

	// ── Rule-based protections ──────────────────────────────────────────
	describe("deny rules (replacing hardcoded protections)", () => {
		const envDenyRules = { allow: [] as string[], deny: ["Write(.env*)", "Edit(.env*)"], ask: [] as string[] };
		const rmDenyRules = { allow: [] as string[], deny: ["Bash(rm *)", "Bash(rm)"], ask: [] as string[] };

		test("denies writes to .env files via deny rule", () => {
			const checker = new PermissionChecker("full-auto", { layers: [envDenyRules] });
			const result = checker.check("write_file", { path: "/project/.env" });
			expect(result.decision).toBe("deny");
			expect(result.reason).toBeDefined();
		});

		test("denies writes to .env.local via deny rule", () => {
			const checker = new PermissionChecker("full-auto", { layers: [envDenyRules] });
			const result = checker.check("write_file", { path: "/project/.env.local" });
			expect(result.decision).toBe("deny");
		});

		test("denies writes to .env.test via deny rule", () => {
			const checker = new PermissionChecker("full-auto", { layers: [envDenyRules] });
			const result = checker.check("edit_file", { path: ".env.test" });
			expect(result.decision).toBe("deny");
		});

		test("allows writes to non-.env files", () => {
			const checker = new PermissionChecker("full-auto", { layers: [envDenyRules] });
			const result = checker.check("write_file", { path: "/project/not-env-file.txt" });
			expect(result.decision).toBe("allow");
		});

		test("denies rm commands via deny rule", () => {
			const checker = new PermissionChecker("full-auto", { layers: [rmDenyRules] });
			const result = checker.check("bash", { command: "rm file.txt" });
			expect(result.decision).toBe("deny");
		});

		test("denies rm -rf commands via deny rule", () => {
			const checker = new PermissionChecker("full-auto", { layers: [rmDenyRules] });
			const result = checker.check("bash", { command: "rm -rf /tmp/stuff" });
			expect(result.decision).toBe("deny");
		});

		test("allows non-rm bash commands with rm deny rules", () => {
			const checker = new PermissionChecker("full-auto", { layers: [rmDenyRules] });
			const result = checker.check("bash", { command: "ls -la" });
			expect(result.decision).toBe("allow");
		});
	});

	// ── Ask rules ──────────────────────────────────────────────────────
	describe("ask rules", () => {
		test("ask rule forces prompt even in full-auto", () => {
			const checker = new PermissionChecker("full-auto", {
				layers: [{ allow: [], deny: [], ask: ["Bash(git push *)"] }],
			});
			const result = checker.check("bash", { command: "git push origin main" });
			expect(result.decision).toBe("ask");
			expect(result.reason).toBeDefined();
		});

		test("ask rule does not affect non-matching commands", () => {
			const checker = new PermissionChecker("full-auto", {
				layers: [{ allow: [], deny: [], ask: ["Bash(git push *)"] }],
			});
			const result = checker.check("bash", { command: "git status" });
			expect(result.decision).toBe("allow");
		});

		test("deny takes priority over ask for same pattern", () => {
			const checker = new PermissionChecker("full-auto", {
				layers: [{ allow: [], deny: ["Bash(rm *)"], ask: ["Bash(rm *)"] }],
			});
			const result = checker.check("bash", { command: "rm foo" });
			expect(result.decision).toBe("deny");
		});
	});

	// ── Allow rules ────────────────────────────────────────────────────
	describe("allow rules", () => {
		test("allow rule overrides mode matrix ask", () => {
			const checker = new PermissionChecker("suggest", {
				layers: [{ allow: ["Bash(bun *)"], deny: [], ask: [] }],
			});
			const result = checker.check("bash", { command: "bun test" });
			expect(result.decision).toBe("allow");
		});

		test("allow rule does not override deny rule", () => {
			const checker = new PermissionChecker("full-auto", {
				layers: [{ allow: ["Write(.env*)"], deny: ["Write(.env*)"], ask: [] }],
			});
			const result = checker.check("write_file", { path: ".env" });
			expect(result.decision).toBe("deny");
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
			expect(checker.check("bash").decision).toBe("ask");
			checker.allowAlways("bash");
			expect(checker.check("bash")).toEqual({ decision: "allow" });
		});

		test("does not bypass deny rules", () => {
			const checker = new PermissionChecker("suggest", {
				layers: [{ allow: [], deny: ["Write(.env*)"], ask: [] }],
			});
			checker.allowAlways("write_file");
			// Normal writes are allowed
			expect(checker.check("write_file", { path: "foo.txt" })).toEqual({ decision: "allow" });
			// .env writes are still denied via deny rule
			const result = checker.check("write_file", { path: ".env" });
			expect(result.decision).toBe("deny");
		});
	});

	// ── Directory scoping ───────────────────────────────────────────────
	describe("directory scoping", () => {
		test("allows writes inside project dir in full-auto", () => {
			const checker = new PermissionChecker("full-auto", { projectDir: "/project" });
			const result = checker.check("write_file", { path: "/project/src/foo.ts" });
			expect(result.decision).toBe("allow");
		});

		test("downgrades allow to ask outside project dir in full-auto", () => {
			const checker = new PermissionChecker("full-auto", { projectDir: "/project" });
			const result = checker.check("write_file", { path: "/other/foo.ts" });
			expect(result.decision).toBe("ask");
		});

		test("ask stays ask outside project dir in suggest mode", () => {
			const checker = new PermissionChecker("suggest", { projectDir: "/project" });
			const result = checker.check("write_file", { path: "/other/foo.ts" });
			expect(result.decision).toBe("ask");
		});

		test("deny stays deny outside project dir", () => {
			const checker = new PermissionChecker("plan", { projectDir: "/project" });
			const result = checker.check("write_file", { path: "/other/foo.ts" });
			expect(result.decision).toBe("deny");
		});

		test("no dir scoping for bash (commands can touch anything)", () => {
			const checker = new PermissionChecker("full-auto", { projectDir: "/project" });
			const result = checker.check("bash", { command: "ls /etc" });
			expect(result.decision).toBe("allow");
		});

		test("no dir scoping for web_fetch", () => {
			const checker = new PermissionChecker("full-auto", { projectDir: "/project" });
			const result = checker.check("web_fetch", { url: "https://example.com" });
			expect(result.decision).toBe("allow");
		});

		test("explicit allow rule overrides dir scoping", () => {
			const checker = new PermissionChecker("full-auto", {
				projectDir: "/project",
				layers: [{ allow: ["Write(/other/**)"], deny: [], ask: [] }],
			});
			const result = checker.check("write_file", { path: "/other/foo.ts" });
			expect(result.decision).toBe("allow");
		});

		test("no projectDir means no scoping", () => {
			const checker = new PermissionChecker("full-auto");
			const result = checker.check("write_file", { path: "/random/path/foo.ts" });
			expect(result.decision).toBe("allow");
		});
	});

	// ── Layer merge precedence ──────────────────────────────────────────
	describe("layer merge", () => {
		test("project allow overrides global deny (more specific layer)", () => {
			const globalLayer = { allow: [] as string[], deny: ["Write(.env*)"], ask: [] as string[] };
			const projectLayer = { allow: ["Write(.env*)"], deny: [] as string[], ask: [] as string[] };
			const checker = new PermissionChecker("full-auto", {
				layers: [globalLayer, projectLayer],
			});
			const result = checker.check("write_file", { path: ".env.local" });
			expect(result.decision).toBe("allow");
		});

		test("project deny overrides global allow", () => {
			const globalLayer = { allow: ["Bash"], deny: [] as string[], ask: [] as string[] };
			const projectLayer = { allow: [] as string[], deny: ["Bash(rm *)"], ask: [] as string[] };
			const checker = new PermissionChecker("full-auto", {
				layers: [globalLayer, projectLayer],
			});
			const result = checker.check("bash", { command: "rm foo" });
			expect(result.decision).toBe("deny");
		});

		test("within same layer, deny wins over allow", () => {
			const checker = new PermissionChecker("full-auto", {
				layers: [{ allow: ["Write(.env*)"], deny: ["Write(.env*)"], ask: [] as string[] }],
			});
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
