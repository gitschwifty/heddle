import { readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import type * as readline from "node:readline";

export function createMentionCompleter(cwd: string): readline.Completer {
	return (line: string): [string[], string] => {
		const words = line.split(/\s+/);
		const lastWord = words[words.length - 1] || "";

		if (!lastWord.startsWith("@")) {
			return [[], line];
		}

		const partial = lastWord.slice(1); // remove @
		let dirPath: string;
		let prefix: string;

		const lastSlash = partial.lastIndexOf("/");
		if (lastSlash !== -1) {
			dirPath = resolve(cwd, partial.slice(0, lastSlash + 1));
			prefix = partial.slice(lastSlash + 1);
		} else {
			dirPath = cwd;
			prefix = partial;
		}

		let entries: string[];
		try {
			entries = readdirSync(dirPath);
		} catch {
			return [[], lastWord];
		}

		const filtered = entries.filter((e) => e.startsWith(prefix));
		const dirPrefix = lastSlash !== -1 ? partial.slice(0, lastSlash + 1) : "";

		const completions = filtered.map((entry) => {
			const fullPath = join(dirPath, entry);
			try {
				const isDir = statSync(fullPath).isDirectory();
				return `@${dirPrefix}${entry}${isDir ? "/" : ""}`;
			} catch {
				return `@${dirPrefix}${entry}`;
			}
		});

		return [completions, lastWord];
	};
}
