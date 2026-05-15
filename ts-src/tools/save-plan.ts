import { Type } from "@sinclair/typebox";
import { savePlan } from "../plans/storage.ts";
import type { HeddleTool } from "./types.ts";

const SavePlanParams = Type.Object({
	name: Type.String({ description: "Name for the plan (used as filename)" }),
	content: Type.String({ description: "The plan content in markdown" }),
});

/** Factory: creates a save_plan tool that persists plans to disk. */
export function createSavePlanTool(sessionId: string, model?: string): HeddleTool {
	return {
		name: "save_plan",
		description: "Save a plan to disk as a markdown file. Plans persist across sessions and can be loaded later.",
		parameters: SavePlanParams,
		async execute(params: unknown): Promise<string> {
			const { name, content } = params as { name: string; content: string };
			const filePath = await savePlan(name, content, {
				model,
				sessionId,
			});
			return `Saved plan "${name}" to ${filePath}`;
		},
	};
}
