import { readFile, writeFile } from "node:fs/promises";
import { Type } from "@sinclair/typebox";
import { cascadingMatch, findClosestMatch } from "./fuzzy-match.ts";
import type { HeddleTool } from "./types.ts";

interface EditParams {
	file_path: string;
	old_string: string;
	new_string: string;
	replace_all?: boolean;
}

export function createEditTool(): HeddleTool {
	return {
		name: "edit_file",
		description:
			"Replace occurrences of old_string with new_string in a file. By default, old_string must appear exactly once (unique match). Set replace_all to true to replace every occurrence.",
		parameters: Type.Object({
			file_path: Type.String({ description: "Path to the file (absolute or relative to cwd)" }),
			old_string: Type.String({ description: "The text to find" }),
			new_string: Type.String({ description: "The replacement text" }),
			replace_all: Type.Optional(Type.Boolean({ description: "Replace all occurrences" })),
		}),
		execute: async (params) => {
			const { file_path, old_string, new_string, replace_all } = params as EditParams;

			let content: string;
			try {
				content = await readFile(file_path, "utf-8");
			} catch {
				return `Error: File not found: ${file_path}`;
			}

			if (content.includes(old_string)) {
				if (replace_all) {
					const count = content.split(old_string).length - 1;
					const updated = content.replaceAll(old_string, new_string);
					await writeFile(file_path, updated, "utf-8");
					return `Replaced ${count} occurrences in ${file_path}`;
				}

				// Unique match check: old_string must appear exactly once
				const firstIdx = content.indexOf(old_string);
				const secondIdx = content.indexOf(old_string, firstIdx + 1);
				if (secondIdx !== -1) {
					return `Error: old_string is not unique in ${file_path} (found multiple matches). Use replace_all: true to replace all, or provide more context.`;
				}

				const updated = content.replace(old_string, new_string);
				await writeFile(file_path, updated, "utf-8");
				return `Applied edit to ${file_path}`;
			}

			// Exact match failed — try fuzzy matching
			const fuzzy = cascadingMatch(content, old_string);
			if (fuzzy && fuzzy.level > 0) {
				const levelNames = ["exact", "whitespace-normalized", "indent-flexible", "line-fuzzy"];
				const updated =
					content.slice(0, fuzzy.startIndex) + new_string + content.slice(fuzzy.startIndex + fuzzy.matchedText.length);
				await writeFile(file_path, updated, "utf-8");
				return `Applied edit to ${file_path} (${levelNames[fuzzy.level]} match)`;
			}

			// All fuzzy levels failed — provide closest match hint
			const closest = findClosestMatch(content, old_string);
			if (closest) {
				return `Error: old_string not found in ${file_path}. Closest match near line ${closest.line}:\n${closest.snippet}`;
			}
			return `Error: old_string not found in ${file_path}`;
		},
	};
}
