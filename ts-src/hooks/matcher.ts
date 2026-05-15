import type { HookContext, ResolvedHookDefinition } from "./types.ts";

/**
 * Check if a hook's matchers match the given context.
 * No matchers = always matches. All matchers use AND logic.
 */
export function matchesHook(hook: ResolvedHookDefinition, context: HookContext): boolean {
	const { matchers } = hook;
	if (!matchers) return true;

	// tool matcher
	if (matchers.tool !== undefined) {
		if (!context.toolName) return false;
		if (Array.isArray(matchers.tool)) {
			if (!matchers.tool.includes(context.toolName)) return false;
		} else {
			if (matchers.tool !== context.toolName) return false;
		}
	}

	// match_path: glob against file_path extracted from toolArgs JSON
	if (matchers.match_path !== undefined) {
		if (!context.toolArgs) return false;
		let filePath: string | undefined;
		try {
			const parsed = JSON.parse(context.toolArgs);
			filePath = parsed.file_path;
		} catch {
			return false;
		}
		if (!filePath) return false;
		const glob = new Bun.Glob(matchers.match_path);
		if (!glob.match(filePath)) return false;
	}

	// match_args: glob against entire toolArgs string
	if (matchers.match_args !== undefined) {
		if (!context.toolArgs) return false;
		const glob = new Bun.Glob(matchers.match_args);
		if (!glob.match(context.toolArgs)) return false;
	}

	// match_input: glob against userInput
	if (matchers.match_input !== undefined) {
		if (!context.userInput) return false;
		const glob = new Bun.Glob(matchers.match_input);
		if (!glob.match(context.userInput)) return false;
	}

	return true;
}
