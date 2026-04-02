import { existsSync, readdirSync, statSync } from "node:fs";
import { extname, join, relative } from "node:path";
import type { DiscoveryResult } from "../config/discovery.ts";
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

/**
 * Load custom commands from discovery levels and legacy paths.
 * Scans both `skills/` and `commands/` subdirs for backward compatibility.
 * Discovery levels are scanned in priority order (deepest first),
 * with later entries overriding earlier ones for same-named commands.
 */
export async function loadCustomCommands(discovery?: DiscoveryResult): Promise<SlashCommand[]> {
	const commandMap = new Map<string, SlashCommand>();

	if (discovery) {
		// Discovery-based loading: iterate levels in reverse (shallowest first)
		// so that deeper/more-specific levels override shallower ones
		const reversedLevels = [...discovery.levels].reverse();
		for (const level of reversedLevels) {
			const baseDir = level.source === "agents" ? level.path : level.path;
			const subdirs = level.source === "agents" ? [baseDir] : [join(baseDir, "skills"), join(baseDir, "commands")];

			for (const dir of subdirs) {
				for (const cmd of scanDirectory(dir)) {
					commandMap.set(cmd.name, cmd);
				}
			}
		}
	} else {
		// Legacy fallback: hardcoded 4-dir scan for backward compatibility
		const heddleHome = getHeddleHome();
		const localDir = getLocalHeddleDir();

		const dirs = [
			join(heddleHome, "skills"),
			join(heddleHome, "commands"),
			join(localDir, "skills"),
			join(localDir, "commands"),
		];

		for (const dir of dirs) {
			for (const cmd of scanDirectory(dir)) {
				commandMap.set(cmd.name, cmd);
			}
		}
	}

	return [...commandMap.values()];
}
