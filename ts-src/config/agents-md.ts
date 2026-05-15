import { readdirSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { getHeddleHome } from "./paths.ts";

/**
 * Find all AGENTS.md files walking up from startDir to home directory.
 * Case-insensitive matching. Also checks HEDDLE_HOME/AGENTS.md.
 * Returns paths ordered farthest-first → closest-last.
 */
export function findAllAgentsMd(startDir?: string): string[] {
	const start = resolve(startDir ?? process.cwd());
	const home = homedir();
	const found: string[] = [];

	// Walk up from startDir toward home (inclusive), or to filesystem root
	let current = start;
	while (true) {
		const match = findAgentsMdIn(current);
		if (match) {
			found.push(match);
		}

		// Stop at home directory — don't go above it
		if (current === home) break;

		const parent = resolve(current, "..");
		// Stop if we've reached the filesystem root (parent === current)
		if (parent === current) break;

		current = parent;
	}

	// Reverse so farthest-first → closest-last
	found.reverse();

	// Check HEDDLE_HOME
	const heddleHome = getHeddleHome();
	const heddleMatch = findAgentsMdIn(heddleHome);
	if (heddleMatch && !found.includes(heddleMatch)) {
		// Prepend HEDDLE_HOME (it's the most global scope)
		found.unshift(heddleMatch);
	}

	return found;
}

/**
 * Look for an AGENTS.md file (case-insensitive) in a directory.
 * Returns the full path if found, undefined otherwise.
 */
function findAgentsMdIn(dir: string): string | undefined {
	try {
		const entries = readdirSync(dir);
		const match = entries.find((entry) => entry.toLowerCase() === "agents.md");
		return match ? join(dir, match) : undefined;
	} catch {
		return undefined;
	}
}

/**
 * Load and concatenate all AGENTS.md files.
 * Returns null if no files found.
 */
export function loadAgentsContext(startDir?: string): string | null {
	const paths = findAllAgentsMd(startDir);
	if (paths.length === 0) return null;

	return paths.map((p) => readFileSync(p, "utf-8")).join("\n\n");
}
