import { mkdirSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { randomUUID } from "node:crypto";

/**
 * Create an isolated test sandbox in $TMPDIR. No user-level side effects.
 * Sets HEDDLE_HOME and chdir to sandbox project dir.
 * Call cleanup() in afterEach/afterAll to restore env and cwd.
 */
export function createTestSandbox(prefix: string) {
	const rawRoot = join(tmpdir(), "heddle-test", `${prefix}-${randomUUID().slice(0, 8)}`);
	mkdirSync(rawRoot, { recursive: true });
	// realpathSync resolves macOS /var → /private/var so cwd comparisons work
	const root = realpathSync(rawRoot);

	const home = join(root, "home");
	const heddleHome = join(home, ".heddle");
	const project = join(root, "project");

	mkdirSync(heddleHome, { recursive: true });
	mkdirSync(project, { recursive: true });

	const origEnv = { ...process.env };
	const origCwd = process.cwd();
	process.env.HEDDLE_HOME = heddleHome;
	process.chdir(project);

	return {
		root,
		home,
		heddleHome,
		project,
		cleanup() {
			process.env = { ...origEnv };
			process.chdir(origCwd);
			rmSync(root, { recursive: true, force: true });
		},
	};
}
