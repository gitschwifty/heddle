import { appendFileSync, existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { Type } from "@sinclair/typebox";
import { getGlobalMemoryDir } from "../config/paths.ts";
import type { HeddleTool } from "./types.ts";

const SaveMemoryParams = Type.Object({
	content: Type.String({ description: "The memory content to save" }),
	scope: Type.Optional(
		Type.Union([Type.Literal("project"), Type.Literal("global")], {
			default: "project",
			description: 'Where to save: "project" (default) or "global"',
		}),
	),
});

/** Factory: creates a save_memory tool that appends to MEMORY.md. */
export function createSaveMemoryTool(memoryDir: string): HeddleTool {
	return {
		name: "save_memory",
		description:
			"Save a memory note to MEMORY.md. Use scope='project' for project-specific notes or scope='global' for cross-project notes.",
		parameters: SaveMemoryParams,
		async execute(params: unknown): Promise<string> {
			const { content, scope = "project" } = params as {
				content: string;
				scope?: "project" | "global";
			};

			const targetDir = scope === "global" ? getGlobalMemoryDir() : memoryDir;
			mkdirSync(targetDir, { recursive: true });

			const filePath = join(targetDir, "MEMORY.md");
			const entry = `\n## ${new Date().toISOString()}\n\n${content}\n`;

			appendFileSync(filePath, entry, "utf-8");

			return `Saved memory to ${scope} MEMORY.md`;
		},
	};
}
