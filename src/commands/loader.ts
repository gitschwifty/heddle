import { existsSync, readdirSync, statSync } from "node:fs";
import { extname, join, relative } from "node:path";
import { getHeddleHome, getLocalHeddleDir } from "../config/paths.ts";
import { appendMessage } from "../session/jsonl.ts";
import type { Message } from "../types.ts";
import type { SlashCommand } from "./types.ts";

function scanDirectory(dir: string, baseDir?: string): SlashCommand[] {
	if (!existsSync(dir)) return [];

	const commands: SlashCommand[] = [];
	const base = baseDir ?? dir;

	for (const entry of readdirSync(dir)) {
		const fullPath = join(dir, entry);
		const stat = statSync(fullPath);

		if (stat.isDirectory()) {
			commands.push(...scanDirectory(fullPath, base));
			continue;
		}

		if (extname(entry) !== ".md") continue;

		const relPath = relative(base, fullPath);
		const name = relPath.replace(/\.md$/, "").replace(/\//g, ":");

		const filePath = fullPath;
		commands.push({
			name,
			description: `Custom command: ${name}`,
			execute: async (args, ctx) => {
				const content = await Bun.file(filePath).text();
				const userContent = args ? `${content}\n\n${args}` : content;
				const msg: Message = { role: "user", content: userContent };
				ctx.messages.push(msg);
				await appendMessage(ctx.sessionFile, msg);
				console.log(`  [skill] ${name} injected`);
			},
		});
	}

	return commands;
}

export async function loadCustomCommands(): Promise<SlashCommand[]> {
	const heddleHome = getHeddleHome();
	const localDir = getLocalHeddleDir();

	// Scan in priority order: global first, local last (overrides)
	const dirs = [
		join(heddleHome, "skills"),
		join(heddleHome, "commands"),
		join(localDir, "skills"),
		join(localDir, "commands"),
	];

	const commandMap = new Map<string, SlashCommand>();

	for (const dir of dirs) {
		for (const cmd of scanDirectory(dir)) {
			commandMap.set(cmd.name, cmd);
		}
	}

	return [...commandMap.values()];
}
