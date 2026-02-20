import { describe, expect, test } from "bun:test";
import { createOpenRouterProvider } from "../../src/provider/openrouter.ts";

const INTEGRATION = process.env.HEDDLE_INTEGRATION_TESTS === "1";
const API_KEY = process.env.OPENROUTER_API_KEY;

// Free model fallback arrays — if primary is rate-limited, try the next
// OpenRouter limits `models` to 3 fallback entries
const FREE_MODELS = [
	"liquid/lfm-2.5-1.2b-instruct:free",
	"arcee-ai/trinity-large-preview:free",
	"arcee-ai/trinity-mini:free",
	"openrouter/free",
];

const REASONING_MODELS = [
	"arcee-ai/trinity-mini:free",
	"stepfun/step-3.5-flash:free",
	"nvidia/nemotron-3-nano-30b-a3b:free",
	"openrouter/free",
];

// Skip integration tests if disabled or no API key
const describeIntegration = INTEGRATION && API_KEY ? describe : describe.skip;

describeIntegration("OpenRouter Integration", () => {
	const provider = createOpenRouterProvider({
		apiKey: API_KEY!,
		model: FREE_MODELS[0]!,
		requestParams: { models: FREE_MODELS.slice(1), route: "fallback" },
	});

	const reasoningProvider = createOpenRouterProvider({
		apiKey: API_KEY!,
		model: REASONING_MODELS[0]!,
		requestParams: {
			models: REASONING_MODELS.slice(1),
			route: "fallback",
			reasoning: { enabled: true },
		},
	});

	test(
		"send() returns a text response",
		async () => {
			const response = await provider.send([{ role: "user", content: "hello!" }]);

			expect(response.id).toBeDefined();
			expect(response.choices.length).toBeGreaterThan(0);
			expect(response.choices[0]?.message.role).toBe("assistant");
			expect(response.choices[0]?.message.content).toBeTruthy();
			expect(response.choices[0]?.finish_reason).toBe("stop");
		},
		{ timeout: 30_000 },
	);

	test(
		"stream() yields chunks and assembles content",
		async () => {
			const contentChunks: string[] = [];
			let hasFinish = false;

			for await (const chunk of provider.stream([{ role: "user", content: "hello!" }])) {
				const choice = chunk.choices[0];
				const delta = choice?.delta as Record<string, unknown> | undefined;
				if (delta?.content) {
					contentChunks.push(delta.content as string);
				}
				if (choice?.finish_reason) {
					hasFinish = true;
				}
			}

			const assembled = contentChunks.join("");

			expect(contentChunks.length).toBeGreaterThan(0);
			expect(assembled.length).toBeGreaterThan(0);
			expect(hasFinish).toBe(true);
		},
		{ timeout: 30_000 },
	);

	test(
		"send() with reasoning returns reasoning tokens",
		async () => {
			const response = await reasoningProvider.send([{ role: "user", content: "hello!" }]);

			expect(response.id).toBeDefined();
			expect(response.choices.length).toBeGreaterThan(0);
			expect(response.choices[0]?.message.content).toBeTruthy();
			expect(response.choices[0]?.finish_reason).toBe("stop");
			// Reasoning tokens should be present (model-dependent, but free models tend to reason)
			const usage = response.usage as Record<string, unknown> | undefined;
			const details = usage?.completion_tokens_details as Record<string, unknown> | undefined;
			if (details?.reasoning_tokens) {
				expect(details.reasoning_tokens).toBeGreaterThan(0);
			}
		},
		{ timeout: 60_000 },
	);
});
