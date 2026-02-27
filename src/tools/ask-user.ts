import { Type } from "@sinclair/typebox";
import type { HeddleTool } from "./types.ts";

export function createAskUserTool(callback: (question: string, options?: string[]) => Promise<string>): HeddleTool {
	return {
		name: "ask_user",
		description: "Ask the user a question. Optionally provide a list of choices.",
		parameters: Type.Object({
			question: Type.String({ description: "The question to ask the user" }),
			options: Type.Optional(Type.Array(Type.String(), { description: "Optional list of choices" })),
		}),
		execute: async (params) => {
			const { question, options } = params as { question: string; options?: string[] };
			try {
				return await callback(question, options);
			} catch (err) {
				return `Error: ${err instanceof Error ? err.message : String(err)}`;
			}
		},
	};
}
