export interface FeatureFlags {
	history: boolean;
	usageData: boolean;
	facets: boolean;
	fileHistory: boolean;
	pasteCache: boolean;
	statusLine: boolean;
	hooks: boolean;
	tasks: boolean;
}

export type Mode = "interactive" | "non-interactive" | "headless";

export const MODE_DEFAULTS: Record<Mode, FeatureFlags> = {
	interactive: {
		history: true,
		usageData: true,
		facets: true,
		fileHistory: true,
		pasteCache: true,
		statusLine: true,
		hooks: true,
		tasks: true,
	},
	"non-interactive": {
		history: false,
		usageData: true,
		facets: true,
		fileHistory: true,
		pasteCache: true,
		statusLine: false,
		hooks: true,
		tasks: true,
	},
	headless: {
		history: false,
		usageData: true,
		facets: false,
		fileHistory: true,
		pasteCache: false,
		statusLine: false,
		hooks: true,
		tasks: true,
	},
};

export function getFeatures(mode: Mode, overrides?: Partial<FeatureFlags>): Readonly<FeatureFlags> {
	const defaults = { ...MODE_DEFAULTS[mode] };
	if (overrides) {
		for (const [key, value] of Object.entries(overrides)) {
			if (value !== undefined) {
				(defaults as Record<string, boolean>)[key] = value;
			}
		}
	}
	return Object.freeze(defaults);
}
