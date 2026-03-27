import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type * as readline from "node:readline";
import { loadCustomCommands } from "../../src/commands/loader.ts";
import type { CommandContext } from "../../src/commands/types.ts";
import type { HeddleConfig } from "../../src/config/loader.ts";
import { CostTracker } from "../../src/cost/tracker.ts";
import type { Provider } from "../../src/provider/types.ts";
import { ToolRegistry } from "../../src/tools/registry.ts";
import type { Message } from "../../src/types.ts";

let tmpDir: string;
const originalEnv = { ...process.env };

beforeAll(() => {
	tmpDir = join(process.env.TMPDIR ?? "/private/tmp/claude-501", `heddle-cmd-loader-${Date.now()}`);
	mkdirSync(tmpDir, { recursive: true });
});

afterAll(() => {
	process.env = { ...originalEnv };
});

function mockContext(overrides?: Partial<CommandContext>): CommandContext {
	return {
		config: { model: "test-model" } as HeddleConfig,
		messages: [{ role: "system", content: "system prompt" }] as Message[],
		registry: new ToolRegistry(),
		costTracker: new CostTracker(),
		sessionFile: join(tmpDir, "session.jsonl"),
		sessionId: "test-session-id",
		provider: {} as Provider,
		rl: { close: () => {} } as unknown as readline.Interface,
		setModel: () => {},
		...overrides,
	};
}

describe("loadCustomCommands", () => {
	test("loads .md files from commands directory", async () => {
		const home = join(tmpDir, "home-cmds");
		const cmdsDir = join(home, "commands");
		mkdirSync(cmdsDir, { recursive: true });
		writeFileSync(join(cmdsDir, "deploy.md"), "Run deployment steps");
		writeFileSync(join(cmdsDir, "review.md"), "Review the code");

		process.env.HEDDLE_HOME = home;
		// Set cwd-based local dir to nonexistent to avoid interference
		const origCwd = process.cwd;
		process.cwd = () => join(tmpDir, "nonexistent-project");

		const commands = await loadCustomCommands();

		process.cwd = origCwd;

		const names = commands.map((c) => c.name);
		expect(names).toContain("deploy");
		expect(names).toContain("review");
	});

	test("subdirectory namespacing with colon", async () => {
		const home = join(tmpDir, "home-ns");
		const subDir = join(home, "commands", "posts");
		mkdirSync(subDir, { recursive: true });
		writeFileSync(join(subDir, "new.md"), "Create a new post");

		process.env.HEDDLE_HOME = home;
		const origCwd = process.cwd;
		process.cwd = () => join(tmpDir, "nonexistent-ns");

		const commands = await loadCustomCommands();

		process.cwd = origCwd;

		const names = commands.map((c) => c.name);
		expect(names).toContain("posts:new");
	});

	test("skips non-.md files", async () => {
		const home = join(tmpDir, "home-skip");
		const cmdsDir = join(home, "commands");
		mkdirSync(cmdsDir, { recursive: true });
		writeFileSync(join(cmdsDir, "valid.md"), "Valid command");
		writeFileSync(join(cmdsDir, "ignored.txt"), "Not a command");
		writeFileSync(join(cmdsDir, "also-ignored.js"), "Not a command");

		process.env.HEDDLE_HOME = home;
		const origCwd = process.cwd;
		process.cwd = () => join(tmpDir, "nonexistent-skip");

		const commands = await loadCustomCommands();

		process.cwd = origCwd;

		expect(commands).toHaveLength(1);
		expect(commands[0]?.name).toBe("valid");
	});

	test("handles missing directories gracefully", async () => {
		process.env.HEDDLE_HOME = join(tmpDir, "totally-missing");
		const origCwd = process.cwd;
		process.cwd = () => join(tmpDir, "also-missing");

		const commands = await loadCustomCommands();

		process.cwd = origCwd;

		expect(commands).toEqual([]);
	});

	test("execute injects content as user message", async () => {
		const home = join(tmpDir, "home-exec");
		const cmdsDir = join(home, "commands");
		mkdirSync(cmdsDir, { recursive: true });
		writeFileSync(join(cmdsDir, "greet.md"), "Say hello to the user");

		process.env.HEDDLE_HOME = home;
		const origCwd = process.cwd;
		process.cwd = () => join(tmpDir, "nonexistent-exec");

		const commands = await loadCustomCommands();

		process.cwd = origCwd;

		const greet = commands.find((c) => c.name === "greet");
		expect(greet).toBeDefined();

		const ctx = mockContext();
		const originalLog = console.log;
		const logs: string[] = [];
		console.log = (...args: unknown[]) => {
			logs.push(args.map(String).join(" "));
		};

		await greet!.execute("", ctx);

		console.log = originalLog;

		expect(ctx.messages).toHaveLength(2);
		expect(ctx.messages[1]?.role).toBe("user");
		expect(ctx.messages[1]?.content).toBe("Say hello to the user");
		expect(logs.some((l) => l.includes("[skill]"))).toBe(true);
	});

	test("execute appends args to content", async () => {
		const home = join(tmpDir, "home-args");
		const cmdsDir = join(home, "commands");
		mkdirSync(cmdsDir, { recursive: true });
		writeFileSync(join(cmdsDir, "prompt.md"), "Base prompt content");

		process.env.HEDDLE_HOME = home;
		const origCwd = process.cwd;
		process.cwd = () => join(tmpDir, "nonexistent-args");

		const commands = await loadCustomCommands();

		process.cwd = origCwd;

		const prompt = commands.find((c) => c.name === "prompt");
		expect(prompt).toBeDefined();

		const ctx = mockContext();
		const originalLog = console.log;
		console.log = () => {};

		await prompt!.execute("extra arguments here", ctx);

		console.log = originalLog;

		expect(ctx.messages[1]?.content).toBe("Base prompt content\n\nextra arguments here");
	});

	test("loads from skills directory too", async () => {
		const home = join(tmpDir, "home-skills");
		const skillsDir = join(home, "skills");
		mkdirSync(skillsDir, { recursive: true });
		writeFileSync(join(skillsDir, "refactor.md"), "Refactor the code");

		process.env.HEDDLE_HOME = home;
		const origCwd = process.cwd;
		process.cwd = () => join(tmpDir, "nonexistent-skills");

		const commands = await loadCustomCommands();

		process.cwd = origCwd;

		const names = commands.map((c) => c.name);
		expect(names).toContain("refactor");
	});

	test("local commands override global commands", async () => {
		const home = join(tmpDir, "home-override");
		const globalCmds = join(home, "commands");
		mkdirSync(globalCmds, { recursive: true });
		writeFileSync(join(globalCmds, "deploy.md"), "Global deploy");

		const localDir = join(tmpDir, "project-override");
		const localCmds = join(localDir, ".heddle", "commands");
		mkdirSync(localCmds, { recursive: true });
		writeFileSync(join(localCmds, "deploy.md"), "Local deploy");

		process.env.HEDDLE_HOME = home;
		const origCwd = process.cwd;
		process.cwd = () => localDir;

		const commands = await loadCustomCommands();

		process.cwd = origCwd;

		const deploy = commands.find((c) => c.name === "deploy");
		expect(deploy).toBeDefined();

		// Execute and check that local content is used
		const ctx = mockContext();
		const originalLog = console.log;
		console.log = () => {};
		await deploy!.execute("", ctx);
		console.log = originalLog;

		expect(ctx.messages[1]?.content).toBe("Local deploy");
	});
});
