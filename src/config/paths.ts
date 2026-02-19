import { join, isAbsolute, resolve } from "node:path";
import { mkdirSync } from "node:fs";
import { homedir } from "node:os";

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

/** Global sessions directory. */
export function getSessionsDir(): string {
	return join(getHeddleHome(), "sessions");
}

/** Global agents directory. */
export function getAgentsDir(): string {
	return join(getHeddleHome(), "agents");
}

/** Global skills directory. */
export function getSkillsDir(): string {
	return join(getHeddleHome(), "skills");
}

/** Create the global heddle directory structure if it doesn't exist. */
export function ensureHeddleDirs(): void {
	const home = getHeddleHome();
	mkdirSync(join(home, "sessions"), { recursive: true });
	mkdirSync(join(home, "agents"), { recursive: true });
	mkdirSync(join(home, "skills"), { recursive: true });
}
