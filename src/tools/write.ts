import { mkdir, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { Type } from "@sinclair/typebox";
import type { HeddleTool } from "./types.ts";

export function createWriteTool(): HeddleTool {
	return {
		name: "write_file",
		description: "Write content to a file, creating parent directories if needed. Overwrites existing files.",
		parameters: Type.Object({
			file_path: Type.String({ description: "Absolute path to the file" }),
			content: Type.String({ description: "Content to write" }),
		}),
		execute: async (params) => {
			const { file_path, content } = params as { file_path: string; content: string };
			try {
				await mkdir(dirname(file_path), { recursive: true });
				await writeFile(file_path, content, "utf-8");
				return `Wrote ${content.length} bytes to ${file_path}`;
			} catch (err) {
				const message = err instanceof Error ? err.message : String(err);
				return `Error: Could not write file: ${message}`;
			}
		},
	};
}
