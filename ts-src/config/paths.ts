import { existsSync, mkdirSync, statSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { isAbsolute, join, resolve } from "node:path";

/** Global heddle config directory. Respects HEDDLE_HOME env var. */
export function getHeddleHome(): string {
	const env = process.env.HEDDLE_HOME;
	if (env) {
		return isAbsolute(env) ? env : resolve(process.cwd(), env);
	}
	return join(homedir(), ".heddle");
}

/** Local .heddle directory in the current project. */
export function getLocalHeddleDir(): string {
	return join(process.cwd(), ".heddle");
}

/**
 * Resolved config.toml path. Prefers local over global.
 * Returns local path if the file exists, otherwise global.
 */
export function getConfigPath(): string {
	const localPath = join(getLocalHeddleDir(), "config.toml");
	const { existsSync } = require("node:fs");
	if (existsSync(localPath)) {
		return localPath;
	}
	return join(getHeddleHome(), "config.toml");
}

/** Encode an absolute path as a dash-separated directory name. */
export function encodePath(absolutePath: string): string {
	// Remove trailing slash, then replace all / with -
	return absolutePath.replace(/\/+$/, "").replace(/\//g, "-");
}

/** Project-specific directory under ~/.heddle/projects/{encoded-path}/. */
export function getProjectDir(projectPath?: string): string {
	const encoded = encodePath(projectPath ?? process.cwd());
	return join(getHeddleHome(), "projects", encoded);
}

/** Sessions directory for the current project. */
export function getProjectSessionsDir(projectPath?: string): string {
	return join(getProjectDir(projectPath), "sessions");
}

/** Global agents directory. */
export function getAgentsDir(): string {
	return join(getHeddleHome(), "agents");
}

/** Global skills directory. */
export function getSkillsDir(): string {
	return join(getHeddleHome(), "skills");
}

/** Global history log file. */
export function getHistoryPath(): string {
	return join(getHeddleHome(), "history.jsonl");
}

/** File history backup base directory for the project. */
export function getFileHistoryDir(projectPath?: string): string {
	return join(getProjectDir(projectPath), "file-history");
}

/** Project-specific memory directory. */
export function getProjectMemoryDir(projectPath?: string): string {
	return join(getProjectDir(projectPath), "memory");
}

/** Global memory directory. */
export function getGlobalMemoryDir(): string {
	return join(getHeddleHome(), "memory");
}

/**
 * Walk up from startDir toward homeDir, collecting `.heddle/` directories.
 * Returns deepest-first ordering. Also includes HEDDLE_HOME if it exists
 * and wasn't already found during the walk.
 */
export function walkUpHeddleDirs(startDir?: string, homeDir?: string): string[] {
	const start = resolve(startDir ?? process.cwd());
	const home = homeDir ?? homedir();
	const found: string[] = [];

	let current = start;
	while (true) {
		const candidate = join(current, ".heddle");
		try {
			const stat = statSync(candidate);
			if (stat.isDirectory()) {
				found.push(candidate);
			}
		} catch {
			// Directory doesn't exist or inaccessible — skip
		}

		if (current === home) break;
		const parent = resolve(current, "..");
		if (parent === current) break;
		current = parent;
	}

	// Include HEDDLE_HOME if set and not already in the list
	const heddleHome = getHeddleHome();
	if (!found.includes(heddleHome)) {
		try {
			const stat = statSync(heddleHome);
			if (stat.isDirectory()) {
				found.push(heddleHome);
			}
		} catch {
			// HEDDLE_HOME doesn't exist — skip
		}
	}

	return found;
}

/**
 * Walk up from startDir looking for `.git` (directory or file, for worktree support).
 * Returns the directory containing `.git`, or undefined if not found.
 */
export function findRepoRoot(startDir?: string): string | undefined {
	let current = resolve(startDir ?? process.cwd());

	while (true) {
		const gitPath = join(current, ".git");
		try {
			statSync(gitPath); // exists as file or directory
			return current;
		} catch {
			// Not found — keep walking
		}

		const parent = resolve(current, "..");
		if (parent === current) break;
		current = parent;
	}

	return undefined;
}

/** Returns the system-level heddle config directory path. */
export function getSystemHeddleDir(): string {
	return "/etc/heddle";
}

/** Create the global heddle directory structure and current project dirs. */
export function ensureHeddleDirs(): void {
	const home = getHeddleHome();
	mkdirSync(join(home, "agents"), { recursive: true });
	mkdirSync(join(home, "skills"), { recursive: true });
	mkdirSync(join(home, "memory"), { recursive: true });
	mkdirSync(getProjectSessionsDir(), { recursive: true });

	// Write default config with permissions if it doesn't exist
	const configPath = join(home, "config.toml");
	if (!existsSync(configPath)) {
		try {
			const { generateDefaultPermissionsToml } = require("../permissions/defaults.ts");
			writeFileSync(configPath, generateDefaultPermissionsToml(), "utf-8");
		} catch {
			// Non-fatal — permissions defaults are a convenience, not a requirement
		}
	}
}
