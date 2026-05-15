import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { ModelPricing } from "../../src/cost/pricing.ts";

const MOCK_MODELS_RESPONSE = {
	data: [
		{
			id: "openai/gpt-4",
			name: "GPT-4",
			pricing: { prompt: "0.00003", completion: "0.00006" },
			context_length: 128000,
			top_provider: { max_completion_tokens: 4096 },
			architecture: { modality: "text+image->text" },
			supported_parameters: ["temperature", "top_p"],
		},
		{
			id: "anthropic/claude-3-opus",
			name: "Claude 3 Opus",
			pricing: { prompt: "0.000015", completion: "0.000075" },
			context_length: 200000,
			top_provider: { max_completion_tokens: 4096 },
			architecture: { modality: "text+image->text" },
			supported_parameters: ["temperature", "top_p", "top_k"],
		},
	],
};

let fetchCount = 0;
let server: ReturnType<typeof Bun.serve>;
let baseUrl: string;

beforeAll(() => {
	server = Bun.serve({
		port: 0,
		fetch(req) {
			const url = new URL(req.url);
			if (url.pathname === "/models") {
				fetchCount++;
				return new Response(JSON.stringify(MOCK_MODELS_RESPONSE), {
					headers: { "Content-Type": "application/json" },
				});
			}
			return new Response("Not found", { status: 404 });
		},
	});
	baseUrl = `http://127.0.0.1:${server.port}`;
});

afterAll(() => {
	server.stop(true);
});

describe("ModelPricing", () => {
	test("getModel returns pricing info for known model", async () => {
		const pricing = new ModelPricing("test-key", baseUrl);
		const model = await pricing.getModel("openai/gpt-4");
		expect(model).toBeDefined();
		expect(model!.id).toBe("openai/gpt-4");
		expect(model!.name).toBe("GPT-4");
		expect(model!.promptPrice).toBe(0.00003);
		expect(model!.completionPrice).toBe(0.00006);
		expect(model!.contextLength).toBe(128000);
		expect(model!.maxCompletionTokens).toBe(4096);
		expect(model!.modality).toBe("text+image->text");
		expect(model!.supportedParameters).toEqual(["temperature", "top_p"]);
	});

	test("getModel returns undefined for unknown model", async () => {
		const pricing = new ModelPricing("test-key", baseUrl);
		const model = await pricing.getModel("nonexistent/model");
		expect(model).toBeUndefined();
	});

	test("getAllModels returns full list", async () => {
		const pricing = new ModelPricing("test-key", baseUrl);
		const models = await pricing.getAllModels();
		expect(models).toHaveLength(2);
		expect(models.map((m) => m.id)).toEqual(["openai/gpt-4", "anthropic/claude-3-opus"]);
	});

	test("lazy loading: no fetch until first access", () => {
		const pricing = new ModelPricing("test-key", baseUrl);
		expect(pricing.isLoaded).toBe(false);
	});

	test("caching: second access does not re-fetch", async () => {
		const countBefore = fetchCount;
		const pricing = new ModelPricing("test-key", baseUrl);
		await pricing.getModel("openai/gpt-4");
		await pricing.getModel("anthropic/claude-3-opus");
		await pricing.getAllModels();
		expect(fetchCount - countBefore).toBe(1);
	});

	test("concurrent fetch deduplication", async () => {
		const countBefore = fetchCount;
		const pricing = new ModelPricing("test-key", baseUrl);
		const [a, b, c] = await Promise.all([
			pricing.getModel("openai/gpt-4"),
			pricing.getAllModels(),
			pricing.estimateCost("openai/gpt-4", 1000, 500),
		]);
		expect(a).toBeDefined();
		expect(b).toHaveLength(2);
		expect(c).not.toBeNull();
		expect(fetchCount - countBefore).toBe(1);
	});

	test("parses string prices to numbers correctly", async () => {
		const pricing = new ModelPricing("test-key", baseUrl);
		const model = await pricing.getModel("anthropic/claude-3-opus");
		expect(model!.promptPrice).toBe(0.000015);
		expect(model!.completionPrice).toBe(0.000075);
	});

	test("estimateCost calculates correctly", async () => {
		const pricing = new ModelPricing("test-key", baseUrl);
		const cost = await pricing.estimateCost("openai/gpt-4", 1000, 500);
		expect(cost).toBeCloseTo(0.06);
	});

	test("estimateCost returns null for unknown model", async () => {
		const pricing = new ModelPricing("test-key", baseUrl);
		const cost = await pricing.estimateCost("nonexistent/model", 1000, 500);
		expect(cost).toBeNull();
	});

	test("isLoaded reflects fetch state", async () => {
		const pricing = new ModelPricing("test-key", baseUrl);
		expect(pricing.isLoaded).toBe(false);
		await pricing.getAllModels();
		expect(pricing.isLoaded).toBe(true);
	});
});
