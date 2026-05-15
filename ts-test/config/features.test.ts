import { describe, expect, test } from "bun:test";
import { type FeatureFlags, getFeatures, MODE_DEFAULTS, type Mode } from "../../src/config/features.ts";

describe("feature flags", () => {
	describe("MODE_DEFAULTS", () => {
		test("interactive mode has all flags true", () => {
			const flags = MODE_DEFAULTS.interactive;
			for (const value of Object.values(flags)) {
				expect(value).toBe(true);
			}
		});

		test("non-interactive disables history and statusLine", () => {
			const flags = MODE_DEFAULTS["non-interactive"];
			expect(flags.history).toBe(false);
			expect(flags.statusLine).toBe(false);
			expect(flags.usageData).toBe(true);
			expect(flags.facets).toBe(true);
			expect(flags.fileHistory).toBe(true);
			expect(flags.pasteCache).toBe(true);
			expect(flags.hooks).toBe(true);
			expect(flags.tasks).toBe(true);
		});

		test("headless disables history, facets, statusLine, pasteCache", () => {
			const flags = MODE_DEFAULTS.headless;
			expect(flags.history).toBe(false);
			expect(flags.facets).toBe(false);
			expect(flags.statusLine).toBe(false);
			expect(flags.pasteCache).toBe(false);
			expect(flags.usageData).toBe(true);
			expect(flags.fileHistory).toBe(true);
			expect(flags.hooks).toBe(true);
			expect(flags.tasks).toBe(true);
		});
	});

	describe("getFeatures", () => {
		test("returns defaults for mode when no overrides", () => {
			const features = getFeatures("interactive");
			expect(features).toEqual(MODE_DEFAULTS.interactive);
		});

		test("merges overrides with mode defaults", () => {
			const features = getFeatures("interactive", { history: false });
			expect(features.history).toBe(false);
			expect(features.usageData).toBe(true);
		});

		test("overrides can enable flags disabled by mode", () => {
			const features = getFeatures("headless", { history: true, facets: true });
			expect(features.history).toBe(true);
			expect(features.facets).toBe(true);
			expect(features.statusLine).toBe(false);
		});

		test("returns frozen object", () => {
			const features = getFeatures("interactive");
			expect(Object.isFrozen(features)).toBe(true);
		});

		test("ignores undefined override values", () => {
			const features = getFeatures("interactive", { history: undefined } as Partial<FeatureFlags>);
			expect(features.history).toBe(true);
		});

		test("all three modes produce valid feature flags", () => {
			const modes: Mode[] = ["interactive", "non-interactive", "headless"];
			for (const mode of modes) {
				const features = getFeatures(mode);
				expect(typeof features.history).toBe("boolean");
				expect(typeof features.usageData).toBe("boolean");
				expect(typeof features.facets).toBe("boolean");
				expect(typeof features.fileHistory).toBe("boolean");
				expect(typeof features.pasteCache).toBe("boolean");
				expect(typeof features.statusLine).toBe("boolean");
				expect(typeof features.hooks).toBe("boolean");
				expect(typeof features.tasks).toBe("boolean");
			}
		});
	});
});
