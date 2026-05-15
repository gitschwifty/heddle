import { Type } from "@sinclair/typebox";
import type { HeddleTool } from "./types.ts";

export function createGlobTool(): HeddleTool {
	return {
		name: "glob",
		description: "Find files matching a glob pattern. Returns matching file paths, one per line.",
		parameters: Type.Object({
			pattern: Type.String({ description: "Glob pattern (e.g. 'src/**/*.ts')" }),
			path: Type.Optional(Type.String({ description: "Directory to search in (defaults to cwd)" })),
		}),
		execute: async (params) => {
			const { pattern, path } = params as { pattern: string; path?: string };
			try {
				const glob = new Bun.Glob(pattern);
				const results: string[] = [];
				for await (const entry of glob.scan({ cwd: path ?? ".", absolute: true })) {
					results.push(entry);
				}
				if (results.length === 0) return "No files matched the pattern.";
				return results.join("\n");
			} catch (err) {
				const message = err instanceof Error ? err.message : String(err);
				return `Error: ${message}`;
			}
		},
	};
}
