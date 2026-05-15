import { describe, expect, test } from "bun:test";
import { loadHooks, mergeHooksWithIpc } from "../../src/hooks/loader.ts";
import type { ResolvedHooksConfig } from "../../src/hooks/types.ts";

describe("loadHooks", () => {
	test("returns empty config when no hooks section", () => {
		const result = loadHooks({}, {});
		expect(result).toEqual({});
	});

	test("parses hooks from global config", () => {
		const global = {
			hooks: {
				pre_tool: [{ command: "lint.sh" }],
			},
		};
		const result = loadHooks(global, {});
		expect(result.pre_tool).toHaveLength(1);
		expect(result.pre_tool![0]!.command).toBe("lint.sh");
	});

	test("parses hooks from local config", () => {
		const local = {
			hooks: {
				post_tool: [{ command: "log.sh" }],
			},
		};
		const result = loadHooks({}, local);
		expect(result.post_tool).toHaveLength(1);
		expect(result.post_tool![0]!.command).toBe("log.sh");
	});

	test("merges additively — global hooks first, local appended", () => {
		const global = {
			hooks: {
				pre_tool: [{ command: "global-lint.sh" }],
			},
		};
		const local = {
			hooks: {
				pre_tool: [{ command: "local-lint.sh" }],
			},
		};
		const result = loadHooks(global, local);
		expect(result.pre_tool).toHaveLength(2);
		expect(result.pre_tool![0]!.command).toBe("global-lint.sh");
		expect(result.pre_tool![1]!.command).toBe("local-lint.sh");
	});

	test("merges different event keys", () => {
		const global = {
			hooks: {
				pre_tool: [{ command: "pre.sh" }],
			},
		};
		const local = {
			hooks: {
				post_tool: [{ command: "post.sh" }],
			},
		};
		const result = loadHooks(global, local);
		expect(result.pre_tool).toHaveLength(1);
		expect(result.post_tool).toHaveLength(1);
	});

	test("applies defaults to hook definitions", () => {
		const raw = {
			hooks: {
				pre_tool: [{ command: "test.sh" }],
			},
		};
		const result = loadHooks(raw, {});
		const hook = result.pre_tool![0]!;
		expect(hook.timeout).toBe(10000);
		expect(hook.mode).toBe("both");
		expect(hook.async).toBe(false);
	});

	test("ignores invalid hooks section (not an object)", () => {
		const result = loadHooks({ hooks: "invalid" }, {});
		expect(result).toEqual({});
	});

	test("ignores invalid event entries (not an array)", () => {
		const result = loadHooks({ hooks: { pre_tool: "not-array" } }, {});
		expect(result).toEqual({});
	});

	test("filters out hook entries without command", () => {
		const raw = {
			hooks: {
				pre_tool: [{ command: "valid.sh" }, { timeout: 5000 }],
			},
		};
		const result = loadHooks(raw, {});
		expect(result.pre_tool).toHaveLength(1);
		expect(result.pre_tool![0]!.command).toBe("valid.sh");
	});
});

describe("mergeHooksWithIpc", () => {
	test("IPC hooks override for same event", () => {
		const fileHooks: ResolvedHooksConfig = {
			pre_tool: [{ command: "file-hook.sh", timeout: 10000, mode: "both", async: false }],
		};
		const ipcHooks: ResolvedHooksConfig = {
			pre_tool: [{ command: "ipc-hook.sh", timeout: 10000, mode: "both", async: false }],
		};
		const result = mergeHooksWithIpc(fileHooks, ipcHooks);
		expect(result.pre_tool).toHaveLength(1);
		expect(result.pre_tool![0]!.command).toBe("ipc-hook.sh");
	});

	test("preserves file hooks for events not in IPC", () => {
		const fileHooks: ResolvedHooksConfig = {
			pre_tool: [{ command: "file-pre.sh", timeout: 10000, mode: "both", async: false }],
			post_tool: [{ command: "file-post.sh", timeout: 10000, mode: "both", async: false }],
		};
		const ipcHooks: ResolvedHooksConfig = {
			pre_tool: [{ command: "ipc-pre.sh", timeout: 10000, mode: "both", async: false }],
		};
		const result = mergeHooksWithIpc(fileHooks, ipcHooks);
		expect(result.pre_tool![0]!.command).toBe("ipc-pre.sh");
		expect(result.post_tool![0]!.command).toBe("file-post.sh");
	});

	test("empty IPC hooks returns file hooks unchanged", () => {
		const fileHooks: ResolvedHooksConfig = {
			pre_tool: [{ command: "file.sh", timeout: 10000, mode: "both", async: false }],
		};
		const result = mergeHooksWithIpc(fileHooks, {});
		expect(result).toEqual(fileHooks);
	});
});
