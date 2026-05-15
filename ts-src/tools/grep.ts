import { Type } from "@sinclair/typebox";
import type { HeddleTool } from "./types.ts";

export function createGrepTool(): HeddleTool {
	return {
		name: "grep",
		description: "Search for a regex pattern in files. Uses ripgrep-style output with file paths and line numbers.",
		parameters: Type.Object({
			pattern: Type.String({ description: "Regex pattern to search for" }),
			path: Type.Optional(Type.String({ description: "File or directory to search in (defaults to cwd)" })),
			glob: Type.Optional(Type.String({ description: "Glob filter for files (e.g. '*.ts')" })),
		}),
		execute: async (params) => {
			const { pattern, path, glob } = params as { pattern: string; path?: string; glob?: string };
			try {
				const args = ["grep", "-rn", "--color=never"];
				if (glob) args.push(`--include=${glob}`);
				args.push(pattern, path ?? ".");

				const proc = Bun.spawn(args, { stdout: "pipe", stderr: "pipe" });
				const stdout = await new Response(proc.stdout).text();
				const exitCode = await proc.exited;

				if (exitCode > 1) {
					const stderr = await new Response(proc.stderr).text();
					return `Error: grep exited with code ${exitCode}: ${stderr}`;
				}
				if (exitCode === 1 || !stdout.trim()) return "No matches found.";
				return stdout.trim();
			} catch (err) {
				const message = err instanceof Error ? err.message : String(err);
				return `Error: ${message}`;
			}
		},
	};
}
