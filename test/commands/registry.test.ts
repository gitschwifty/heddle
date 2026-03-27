import { describe, expect, test } from "bun:test";
import { CommandRegistry } from "../../src/commands/registry.ts";
import type { SlashCommand } from "../../src/commands/types.ts";

function makeCommand(name: string, description = `Test: ${name}`): SlashCommand {
	return {
		name,
		description,
		execute: async () => {},
	};
}

describe("CommandRegistry", () => {
	test("register and get a command", () => {
		const reg = new CommandRegistry();
		const cmd = makeCommand("help");
		reg.register(cmd);
		expect(reg.get("help")).toBe(cmd);
	});

	test("get returns undefined for unknown command", () => {
		const reg = new CommandRegistry();
		expect(reg.get("nope")).toBeUndefined();
	});

	test("all() returns all registered commands", () => {
		const reg = new CommandRegistry();
		reg.register(makeCommand("help"));
		reg.register(makeCommand("exit"));
		reg.register(makeCommand("cost"));

		const all = reg.all();
		expect(all).toHaveLength(3);
		expect(all.map((c) => c.name)).toEqual(["help", "exit", "cost"]);
	});

	test("suggest() returns closest match for typo", () => {
		const reg = new CommandRegistry();
		reg.register(makeCommand("help"));
		reg.register(makeCommand("exit"));
		reg.register(makeCommand("status"));

		expect(reg.suggest("halp")).toBe("help");
		expect(reg.suggest("staus")).toBe("status");
	});

	test("suggest() returns undefined when no close match", () => {
		const reg = new CommandRegistry();
		reg.register(makeCommand("help"));

		expect(reg.suggest("zzzzzzzzz")).toBeUndefined();
	});

	test("later registration overrides earlier", () => {
		const reg = new CommandRegistry();
		const global = makeCommand("deploy", "Global deploy");
		const local = makeCommand("deploy", "Local deploy");

		reg.register(global);
		reg.register(local);

		const result = reg.get("deploy");
		expect(result).toBe(local);
		expect(result?.description).toBe("Local deploy");
	});

	test("override does not duplicate in all()", () => {
		const reg = new CommandRegistry();
		reg.register(makeCommand("deploy", "Global"));
		reg.register(makeCommand("deploy", "Local"));

		expect(reg.all()).toHaveLength(1);
		expect(reg.all()[0]?.description).toBe("Local");
	});
});
