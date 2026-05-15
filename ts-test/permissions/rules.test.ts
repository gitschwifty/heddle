import { describe, expect, test } from "bun:test";
import {
	evaluateRules,
	matchRule,
	mergeConfigs,
	type PermissionConfig,
	type PermissionRule,
	parseRule,
} from "../../src/permissions/rules.ts";

describe("parseRule", () => {
	test("bare tool name", () => {
		const result = parseRule("Read");
		expect(result).toEqual({ tool: "read_file" });
	});

	test("tool name with pattern", () => {
		const result = parseRule("Write(src/**)");
		expect(result).toEqual({ tool: "write_file", pattern: "src/**" });
	});

	test("tool name with .env* pattern", () => {
		const result = parseRule("Write(.env*)");
		expect(result).toEqual({ tool: "write_file", pattern: ".env*" });
	});

	test("Bash with command pattern", () => {
		const result = parseRule("Bash(rm *)");
		expect(result).toEqual({ tool: "bash", pattern: "rm *" });
	});

	test("Bash bare name", () => {
		const result = parseRule("Bash");
		expect(result).toEqual({ tool: "bash" });
	});

	test("Edit maps to edit_file", () => {
		const result = parseRule("Edit(*.ts)");
		expect(result).toEqual({ tool: "edit_file", pattern: "*.ts" });
	});

	test("Glob maps to glob", () => {
		const result = parseRule("Glob");
		expect(result).toEqual({ tool: "glob" });
	});

	test("Grep maps to grep", () => {
		const result = parseRule("Grep");
		expect(result).toEqual({ tool: "grep" });
	});

	test("WebFetch maps to web_fetch", () => {
		const result = parseRule("WebFetch(*.npmjs.org)");
		expect(result).toEqual({ tool: "web_fetch", pattern: "*.npmjs.org" });
	});

	test("category name expands to all tools in category", () => {
		const result = parseRule("write");
		expect(Array.isArray(result)).toBe(true);
		const rules = result as PermissionRule[];
		const tools = rules.map((r) => r.tool);
		expect(tools).toContain("write_file");
		expect(tools).toContain("edit_file");
		expect(tools).toContain("save_memory");
	});

	test("category name with pattern applies to all tools", () => {
		const result = parseRule("write(src/**)");
		expect(Array.isArray(result)).toBe(true);
		const rules = result as PermissionRule[];
		for (const rule of rules) {
			expect(rule.pattern).toBe("src/**");
		}
	});

	test("read category expands", () => {
		const result = parseRule("read");
		expect(Array.isArray(result)).toBe(true);
		const rules = result as PermissionRule[];
		const tools = rules.map((r) => r.tool);
		expect(tools).toContain("read_file");
		expect(tools).toContain("glob");
		expect(tools).toContain("grep");
	});

	test("execute category expands to bash", () => {
		const result = parseRule("execute");
		expect(Array.isArray(result)).toBe(true);
		const rules = result as PermissionRule[];
		expect(rules).toEqual([{ tool: "bash" }]);
	});

	test("network category expands to web_fetch", () => {
		const result = parseRule("network");
		expect(Array.isArray(result)).toBe(true);
		const rules = result as PermissionRule[];
		expect(rules).toEqual([{ tool: "web_fetch" }]);
	});

	test("wildcard * matches all tools", () => {
		const result = parseRule("*");
		expect(result).toEqual({ tool: "*" });
	});

	test("invalid string returns null", () => {
		const result = parseRule("");
		expect(result).toBeNull();
	});

	test("unclosed paren returns null", () => {
		const result = parseRule("Write(src/**");
		expect(result).toBeNull();
	});

	test("case-insensitive tool name matching", () => {
		const result = parseRule("write_file(src/**)");
		expect(result).toEqual({ tool: "write_file", pattern: "src/**" });
	});

	test("already-snake_case tool name", () => {
		const result = parseRule("read_file");
		expect(result).toEqual({ tool: "read_file" });
	});
});

describe("matchRule", () => {
	test("exact tool name match, no pattern", () => {
		expect(matchRule({ tool: "bash" }, "bash")).toBe(true);
	});

	test("exact tool name, no match", () => {
		expect(matchRule({ tool: "bash" }, "read_file")).toBe(false);
	});

	test("wildcard tool matches any tool", () => {
		expect(matchRule({ tool: "*" }, "bash")).toBe(true);
		expect(matchRule({ tool: "*" }, "write_file")).toBe(true);
	});

	test("glob pattern matches file path", () => {
		expect(matchRule({ tool: "write_file", pattern: "src/**" }, "write_file", { path: "src/foo/bar.ts" })).toBe(true);
	});

	test("glob pattern does not match different path", () => {
		expect(matchRule({ tool: "write_file", pattern: "src/**" }, "write_file", { path: "test/foo.ts" })).toBe(false);
	});

	test("basename matching for .env*", () => {
		expect(matchRule({ tool: "write_file", pattern: ".env*" }, "write_file", { path: "/project/dir/.env.local" })).toBe(
			true,
		);
	});

	test("basename matching for *.pem", () => {
		expect(matchRule({ tool: "write_file", pattern: "*.pem" }, "write_file", { path: "/home/user/cert.pem" })).toBe(
			true,
		);
	});

	test("command pattern matching for bash", () => {
		expect(matchRule({ tool: "bash", pattern: "rm *" }, "bash", { command: "rm -rf /tmp" })).toBe(true);
	});

	test("command pattern does not match different command", () => {
		expect(matchRule({ tool: "bash", pattern: "rm *" }, "bash", { command: "ls -la" })).toBe(false);
	});

	test("bare rm pattern matches bare rm command", () => {
		expect(matchRule({ tool: "bash", pattern: "rm" }, "bash", { command: "rm" })).toBe(true);
	});

	test("bare rm pattern does not match rm with args", () => {
		expect(matchRule({ tool: "bash", pattern: "rm" }, "bash", { command: "rm file.txt" })).toBe(false);
	});

	test("rule with pattern but no args - no match", () => {
		expect(matchRule({ tool: "write_file", pattern: "src/**" }, "write_file")).toBe(false);
	});

	test("host pattern matching for web_fetch", () => {
		expect(
			matchRule({ tool: "web_fetch", pattern: "*.npmjs.org" }, "web_fetch", {
				url: "https://registry.npmjs.org/package",
			}),
		).toBe(true);
	});

	test("host pattern does not match different host", () => {
		expect(
			matchRule({ tool: "web_fetch", pattern: "*.npmjs.org" }, "web_fetch", {
				url: "https://example.com/foo",
			}),
		).toBe(false);
	});

	test("sudo * matches sudo commands", () => {
		expect(matchRule({ tool: "bash", pattern: "sudo *" }, "bash", { command: "sudo rm -rf /" })).toBe(true);
	});

	test("chmod * matches chmod commands", () => {
		expect(matchRule({ tool: "bash", pattern: "chmod *" }, "bash", { command: "chmod 777 file" })).toBe(true);
	});
});

describe("evaluateRules", () => {
	test("deny wins over allow at same layer", () => {
		const config: PermissionConfig = {
			allow: [{ tool: "write_file" }],
			deny: [{ tool: "write_file", pattern: ".env*" }],
			ask: [],
		};
		const result = evaluateRules(config, "write_file", { path: ".env" });
		expect(result).toBe("deny");
	});

	test("allow applies when no deny match", () => {
		const config: PermissionConfig = {
			allow: [{ tool: "write_file" }],
			deny: [{ tool: "write_file", pattern: ".env*" }],
			ask: [],
		};
		const result = evaluateRules(config, "write_file", { path: "src/foo.ts" });
		expect(result).toBe("allow");
	});

	test("ask takes precedence over allow", () => {
		const config: PermissionConfig = {
			allow: [{ tool: "bash" }],
			deny: [],
			ask: [{ tool: "bash", pattern: "git push *" }],
		};
		const result = evaluateRules(config, "bash", { command: "git push origin main" });
		expect(result).toBe("ask");
	});

	test("deny takes precedence over ask", () => {
		const config: PermissionConfig = {
			allow: [],
			deny: [{ tool: "bash", pattern: "rm *" }],
			ask: [{ tool: "bash", pattern: "rm *" }],
		};
		const result = evaluateRules(config, "bash", { command: "rm file.txt" });
		expect(result).toBe("deny");
	});

	test("null when no rules match", () => {
		const config: PermissionConfig = {
			allow: [{ tool: "read_file" }],
			deny: [],
			ask: [],
		};
		const result = evaluateRules(config, "write_file", { path: "foo.ts" });
		expect(result).toBeNull();
	});

	test("empty config returns null", () => {
		const config: PermissionConfig = { allow: [], deny: [], ask: [] };
		const result = evaluateRules(config, "bash", { command: "ls" });
		expect(result).toBeNull();
	});
});

describe("mergeConfigs", () => {
	test("stacks rules from multiple layers", () => {
		const global: PermissionConfig = {
			allow: [{ tool: "read_file" }],
			deny: [{ tool: "write_file", pattern: ".env*" }],
			ask: [],
		};
		const local: PermissionConfig = {
			allow: [{ tool: "bash", pattern: "bun *" }],
			deny: [],
			ask: [{ tool: "bash", pattern: "git push *" }],
		};
		const merged = mergeConfigs(global, local);
		expect(merged.allow).toHaveLength(2);
		expect(merged.deny).toHaveLength(1);
		expect(merged.ask).toHaveLength(1);
	});

	test("more specific layer allow overrides less specific deny", () => {
		const global: PermissionConfig = {
			allow: [],
			deny: [{ tool: "write_file", pattern: ".env*" }],
			ask: [],
		};
		const local: PermissionConfig = {
			allow: [{ tool: "write_file", pattern: ".env*" }],
			deny: [],
			ask: [],
		};
		// When merging, local allow for same pattern overrides global deny
		const merged = mergeConfigs(global, local);
		const result = evaluateRules(merged, "write_file", { path: ".env.local" });
		expect(result).toBe("allow");
	});

	test("more specific layer deny overrides less specific allow", () => {
		const global: PermissionConfig = {
			allow: [{ tool: "bash" }],
			deny: [],
			ask: [],
		};
		const local: PermissionConfig = {
			allow: [],
			deny: [{ tool: "bash", pattern: "rm *" }],
			ask: [],
		};
		const merged = mergeConfigs(global, local);
		const result = evaluateRules(merged, "bash", { command: "rm file.txt" });
		expect(result).toBe("deny");
	});

	test("single config returns equivalent", () => {
		const config: PermissionConfig = {
			allow: [{ tool: "read_file" }],
			deny: [{ tool: "bash", pattern: "rm *" }],
			ask: [],
		};
		const merged = mergeConfigs(config);
		expect(evaluateRules(merged, "read_file")).toBe("allow");
		expect(evaluateRules(merged, "bash", { command: "rm foo" })).toBe("deny");
	});

	test("empty merge returns empty config", () => {
		const merged = mergeConfigs();
		expect(merged.allow).toHaveLength(0);
		expect(merged.deny).toHaveLength(0);
		expect(merged.ask).toHaveLength(0);
	});
});
