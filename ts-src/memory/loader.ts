import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { getGlobalMemoryDir, getProjectMemoryDir } from "../config/paths.ts";

/**
 * Load memory context from global and project-specific MEMORY.md files.
 * Returns concatenated content with headers, or null if no memory exists.
 */
export function loadMemoryContext(projectPath?: string): string | null {
	const globalPath = join(getGlobalMemoryDir(), "MEMORY.md");
	const projectMemPath = join(getProjectMemoryDir(projectPath), "MEMORY.md");

	const globalContent = readMemoryFile(globalPath);
	const projectContent = readMemoryFile(projectMemPath);

	if (!globalContent && !projectContent) {
		return null;
	}

	const sections: string[] = [];

	if (globalContent) {
		sections.push(`## Global Memory\n${globalContent}`);
	}

	if (projectContent) {
		sections.push(`## Project Memory\n${projectContent}`);
	}

	return sections.join("\n\n");
}

function readMemoryFile(path: string): string | null {
	if (!existsSync(path)) {
		return null;
	}
	const content = readFileSync(path, "utf-8").trim();
	return content || null;
}
