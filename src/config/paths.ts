import { mkdirSync } from "node:fs";
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

/** Create the global heddle directory structure and current project dirs. */
export function ensureHeddleDirs(): void {
	const home = getHeddleHome();
	mkdirSync(join(home, "agents"), { recursive: true });
	mkdirSync(join(home, "skills"), { recursive: true });
	mkdirSync(getProjectSessionsDir(), { recursive: true });
}
