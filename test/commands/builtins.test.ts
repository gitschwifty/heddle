import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import type * as readline from "node:readline";
// Will import once implemented
import { createBuiltinCommands } from "../../src/commands/builtins.ts";
import { CommandRegistry } from "../../src/commands/registry.ts";
import type { CommandContext, SlashCommand } from "../../src/commands/types.ts";
import type { HeddleConfig } from "../../src/config/loader.ts";
import { CostTracker } from "../../src/cost/tracker.ts";
import type { Provider } from "../../src/provider/types.ts";
import { ToolRegistry } from "../../src/tools/registry.ts";
import type { Message } from "../../src/types.ts";

function mockContext(overrides?: Partial<CommandContext>): CommandContext {
	return {
		config: { model: "test-model" } as HeddleConfig,
		messages: [{ role: "system", content: "system prompt" }] as Message[],
		registry: new ToolRegistry(),
		costTracker: new CostTracker(),
		sessionFile: "/tmp/test-session.jsonl",
		sessionId: "test-session-id",
		provider: {} as Provider,
		agentDefinitions: new Map(),
		rl: { close: () => {} } as unknown as readline.Interface,
		setModel: () => {},
		...overrides,
	};
}

function findCommand(commands: SlashCommand[], name: string): SlashCommand {
	const cmd = commands.find((c) => c.name === name);
	if (!cmd) throw new Error(`Command "${name}" not found in builtins`);
	return cmd;
}

describe("builtin commands", () => {
	let logs: string[];
	const originalLog = console.log;

	beforeEach(() => {
		logs = [];
		console.log = (...args: unknown[]) => {
			logs.push(args.map(String).join(" "));
		};
	});

	afterEach(() => {
		console.log = originalLog;
	});

	test("/help lists all registered commands", async () => {
		const reg = new CommandRegistry();
		const builtins = createBuiltinCommands(reg);
		for (const cmd of builtins) reg.register(cmd);

		const help = findCommand(builtins, "help");
		await help.execute("", mockContext());

		expect(logs.length).toBeGreaterThan(0);
		expect(logs.some((l) => l.includes("/help"))).toBe(true);
		expect(logs.some((l) => l.includes("/exit"))).toBe(true);
		expect(logs.some((l) => l.includes("/cost"))).toBe(true);
	});

	test("/clear resets messages to length 1", async () => {
		const messages: Message[] = [
			{ role: "system", content: "system" },
			{ role: "user", content: "hello" },
			{ role: "assistant", content: "hi" },
		];
		const ctx = mockContext({ messages });
		const reg = new CommandRegistry();
		const builtins = createBuiltinCommands(reg);
		const clear = findCommand(builtins, "clear");

		await clear.execute("", ctx);

		expect(ctx.messages).toHaveLength(1);
		expect(ctx.messages[0]?.role).toBe("system");
		expect(logs.some((l) => l.includes("cleared"))).toBe(true);
	});

	test("/exit closes readline", async () => {
		let closed = false;
		const ctx = mockContext({
			rl: {
				close: () => {
					closed = true;
				},
			} as unknown as readline.Interface,
		});
		const reg = new CommandRegistry();
		const builtins = createBuiltinCommands(reg);
		const exit = findCommand(builtins, "exit");

		await exit.execute("", ctx);

		expect(closed).toBe(true);
		expect(logs.some((l) => l.includes("Goodbye"))).toBe(true);
	});

	test("/quit closes readline", async () => {
		let closed = false;
		const ctx = mockContext({
			rl: {
				close: () => {
					closed = true;
				},
			} as unknown as readline.Interface,
		});
		const reg = new CommandRegistry();
		const builtins = createBuiltinCommands(reg);
		const quit = findCommand(builtins, "quit");

		await quit.execute("", ctx);

		expect(closed).toBe(true);
	});

	test("/cost prints token stats", async () => {
		const tracker = new CostTracker();
		tracker.addUsage({
			prompt_tokens: 100,
			completion_tokens: 50,
			total_tokens: 150,
			cost: 0.0025,
		});
		const ctx = mockContext({ costTracker: tracker });
		const reg = new CommandRegistry();
		const builtins = createBuiltinCommands(reg);
		const cost = findCommand(builtins, "cost");

		await cost.execute("", ctx);

		expect(logs.some((l) => l.includes("100"))).toBe(true);
		expect(logs.some((l) => l.includes("50"))).toBe(true);
		expect(logs.some((l) => l.includes("$0.0025"))).toBe(true);
	});

	test("/cost shows N/A when no cost data", async () => {
		const tracker = new CostTracker();
		const ctx = mockContext({ costTracker: tracker });
		const reg = new CommandRegistry();
		const builtins = createBuiltinCommands(reg);
		const cost = findCommand(builtins, "cost");

		await cost.execute("", ctx);

		expect(logs.some((l) => l.includes("N/A"))).toBe(true);
	});

	test("/status prints model and session info", async () => {
		const ctx = mockContext({
			config: { model: "gpt-4", approvalMode: "plan" } as HeddleConfig,
			sessionFile: "/tmp/my-session.jsonl",
			messages: [
				{ role: "system", content: "sys" },
				{ role: "user", content: "hi" },
			] as Message[],
		});
		const reg = new CommandRegistry();
		const builtins = createBuiltinCommands(reg);
		const status = findCommand(builtins, "status");

		await status.execute("", ctx);

		expect(logs.some((l) => l.includes("gpt-4"))).toBe(true);
		expect(logs.some((l) => l.includes("/tmp/my-session.jsonl"))).toBe(true);
		expect(logs.some((l) => l.includes("2"))).toBe(true);
		expect(logs.some((l) => l.includes("plan"))).toBe(true);
	});

	test("/context prints message count and token estimate", async () => {
		const ctx = mockContext({
			messages: [
				{ role: "system", content: "a".repeat(400) },
				{ role: "user", content: "b".repeat(400) },
			] as Message[],
		});
		const reg = new CommandRegistry();
		const builtins = createBuiltinCommands(reg);
		const context = findCommand(builtins, "context");

		await context.execute("", ctx);

		expect(logs.some((l) => l.includes("2"))).toBe(true);
		// 800 chars / 4 = 200 tokens
		expect(logs.some((l) => l.includes("200"))).toBe(true);
	});

	test("/model with args calls setModel", async () => {
		let modelSet = "";
		const ctx = mockContext({
			setModel: (m) => {
				modelSet = m;
			},
		});
		const reg = new CommandRegistry();
		const builtins = createBuiltinCommands(reg);
		const model = findCommand(builtins, "model");

		await model.execute("openrouter/free", ctx);

		expect(modelSet).toBe("openrouter/free");
		expect(logs.some((l) => l.includes("openrouter/free"))).toBe(true);
	});

	test("/model with no args prints current model", async () => {
		const ctx = mockContext({
			config: { model: "current-model" } as HeddleConfig,
		});
		const reg = new CommandRegistry();
		const builtins = createBuiltinCommands(reg);
		const model = findCommand(builtins, "model");

		await model.execute("", ctx);

		expect(logs.some((l) => l.includes("current-model"))).toBe(true);
	});

	test("/tools lists registered tools", async () => {
		const toolRegistry = new ToolRegistry();
		const { Type } = await import("@sinclair/typebox");
		toolRegistry.register({
			name: "read_file",
			description: "Read a file",
			parameters: Type.Object({}),
			execute: async () => "ok",
		});
		toolRegistry.register({
			name: "write_file",
			description: "Write a file",
			parameters: Type.Object({}),
			execute: async () => "ok",
		});
		const ctx = mockContext({ registry: toolRegistry });
		const reg = new CommandRegistry();
		const builtins = createBuiltinCommands(reg);
		const tools = findCommand(builtins, "tools");

		await tools.execute("", ctx);

		expect(logs.some((l) => l.includes("read_file"))).toBe(true);
		expect(logs.some((l) => l.includes("write_file"))).toBe(true);
	});
});
