import { existsSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { findRepoRoot, getSystemHeddleDir, walkUpHeddleDirs } from "./paths.ts";

export interface DiscoveryLevel {
	path: string;
	source: "heddle" | "agents" | "system";
	skills: string[];
	agents: string[];
	config?: string;
}

export interface DiscoveryResult {
	levels: DiscoveryLevel[];
}

/**
 * List files in a subdirectory, returning filenames (not full paths).
 * Returns empty array if the directory doesn't exist or is inaccessible.
 */
function listSubdir(base: string, subdir: string): string[] {
	const dir = join(base, subdir);
	try {
		return readdirSync(dir).filter((entry) => {
			try {
				return statSync(join(dir, entry)).isFile();
			} catch {
				return false;
			}
		});
	} catch {
		return [];
	}
}

/**
 * Build a DiscoveryLevel from a .heddle/ directory path.
 */
function buildHeddleLevel(heddlePath: string): DiscoveryLevel {
	const skills = listSubdir(heddlePath, "skills");
	const agents = listSubdir(heddlePath, "agents");
	const configPath = join(heddlePath, "config.toml");
	const config = existsSync(configPath) ? configPath : undefined;

	return {
		path: heddlePath,
		source: "heddle",
		skills,
		agents,
		config,
	};
}

/**
 * Resolve all discovery levels from the current working directory.
 *
 * Content priority (order in levels array):
 *   1. Deepest .heddle/ first
 *   2. Shallowest .heddle/ (including HEDDLE_HOME)
 *   3. .agents/skills/ at repo root
 *   4. /etc/heddle/ (system)
 *
 * Config priority is handled by consumers — /etc/heddle first (admin override),
 * then deepest .heddle/ through shallowest.
 */
export function resolveDiscovery(cwd?: string, homeDir?: string): DiscoveryResult {
	const levels: DiscoveryLevel[] = [];

	// 1. Walk up from cwd to home, collecting .heddle/ dirs (deepest-first)
	const heddleDirs = walkUpHeddleDirs(cwd, homeDir);
	for (const dir of heddleDirs) {
		levels.push(buildHeddleLevel(dir));
	}

	// 2. Check repo root for .agents/skills/
	const repoRoot = findRepoRoot(cwd);
	if (repoRoot) {
		const agentsSkillsDir = join(repoRoot, ".agents", "skills");
		try {
			const stat = statSync(agentsSkillsDir);
			if (stat.isDirectory()) {
				const skills = listSubdir(join(repoRoot, ".agents"), "skills");
				levels.push({
					path: agentsSkillsDir,
					source: "agents",
					skills,
					agents: [],
				});
			}
		} catch {
			// .agents/skills/ doesn't exist — skip
		}
	}

	// 3. Check /etc/heddle/ (system level — catch EACCES gracefully)
	const systemDir = getSystemHeddleDir();
	try {
		const stat = statSync(systemDir);
		if (stat.isDirectory()) {
			const skills = listSubdir(systemDir, "skills");
			const agents = listSubdir(systemDir, "agents");
			const configPath = join(systemDir, "config.toml");
			const config = existsSync(configPath) ? configPath : undefined;
			levels.push({
				path: systemDir,
				source: "system",
				skills,
				agents,
				config,
			});
		}
	} catch {
		// /etc/heddle doesn't exist or EACCES — skip
	}

	return { levels };
}
