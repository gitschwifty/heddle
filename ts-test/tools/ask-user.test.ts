import { describe, expect, test } from "bun:test";
import { createAskUserTool } from "../../src/tools/ask-user.ts";

describe("ask_user tool", () => {
	test("returns callback result as tool output", async () => {
		const tool = createAskUserTool(async () => "user said yes");
		const result = await tool.execute({ question: "Continue?" });
		expect(result).toBe("user said yes");
	});

	test("passes options to callback when provided", async () => {
		let receivedQuestion = "";
		let receivedOptions: string[] | undefined;

		const tool = createAskUserTool(async (question, options) => {
			receivedQuestion = question;
			receivedOptions = options;
			return "option A";
		});

		await tool.execute({ question: "Pick one", options: ["A", "B", "C"] });
		expect(receivedQuestion).toBe("Pick one");
		expect(receivedOptions).toEqual(["A", "B", "C"]);
	});

	test("handles callback error gracefully", async () => {
		const tool = createAskUserTool(async () => {
			throw new Error("readline broken");
		});
		const result = await tool.execute({ question: "Hello?" });
		expect(result).toContain("Error");
		expect(result).toContain("readline broken");
	});

	test("works with no options (undefined)", async () => {
		let receivedOptions: string[] | undefined = ["should be overwritten"];

		const tool = createAskUserTool(async (_question, options) => {
			receivedOptions = options;
			return "ok";
		});

		await tool.execute({ question: "What?" });
		expect(receivedOptions).toBeUndefined();
	});
});
