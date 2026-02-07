import { describe, expect, test } from "bun:test";
import { createOpenRouterProvider } from "../../src/provider/openrouter.ts";

const API_KEY = process.env.OPENROUTER_API_KEY;
const TEST_MODEL = process.env.TEST_MODEL ?? "openrouter/pony-alpha";

// Skip integration tests if no API key
const describeIntegration = API_KEY ? describe : describe.skip;

describeIntegration("OpenRouter Integration", () => {
	const provider = createOpenRouterProvider({
		apiKey: API_KEY!,
		model: TEST_MODEL,
	});

	test(
		"send() returns a text response",
		async () => {
			const response = await provider.send([{ role: "user", content: "Say hello in exactly 3 words." }]);

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
			const chunks: string[] = [];
			let hasFinish = false;

			for await (const chunk of provider.stream([
				{ role: "user", content: "Say hello in exactly 3 words." },
			])) {
				const choice = chunk.choices[0];
				if (choice?.delta.content) {
					chunks.push(choice.delta.content);
				}
				if (choice?.finish_reason) {
					hasFinish = true;
				}
			}

			expect(chunks.length).toBeGreaterThan(0);
			const assembled = chunks.join("");
			expect(assembled.length).toBeGreaterThan(0);
			expect(hasFinish).toBe(true);
		},
		{ timeout: 30_000 },
	);
});
