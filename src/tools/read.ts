import { readFile } from "node:fs/promises";
import { Type } from "@sinclair/typebox";
import type { HeddleTool } from "./types.ts";

export function createReadTool(): HeddleTool {
	return {
		name: "read_file",
		description: "Read the contents of a file at the given path.",
		parameters: Type.Object({
			file_path: Type.String({ description: "Absolute path to the file" }),
		}),
		execute: async (params) => {
			const { file_path } = params as { file_path: string };
			try {
				return await readFile(file_path, "utf-8");
			} catch {
				return `Error: Could not read file: ${file_path}`;
			}
		},
	};
}
