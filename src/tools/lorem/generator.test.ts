import { describe, expect, it } from "vitest";
import {
  generateParagraphs,
  PARAGRAPH_MAX_SENTENCES,
  PARAGRAPH_MIN_SENTENCES,
  SENTENCE_MAX_WORDS,
  SENTENCE_MIN_WORDS,
} from "./generator";

// Split a paragraph back into sentence strings. Sentences end in `.`; the last
// one keeps its trailing period after split, which we drop before counting
// words so an interior comma doesn't show up as its own token.
function splitSentences(paragraph: string): string[] {
  return paragraph
    .split(". ")
    .map((s, i, arr) => (i === arr.length - 1 ? s.replace(/\.$/, "") : s))
    .filter((s) => s.length > 0);
}

function countWords(sentence: string): number {
  return sentence.replace(/,/g, "").split(/\s+/).filter(Boolean).length;
}

describe("generateParagraphs", () => {
  it("returns the requested number of paragraphs separated by blank lines", () => {
    const text = generateParagraphs(5);
    const paragraphs = text.split("\n\n");
    expect(paragraphs).toHaveLength(5);
    expect(paragraphs.every((p) => p.length > 0)).toBe(true);
  });

  it("each paragraph has 4-7 sentences", () => {
    const paragraphs = generateParagraphs(5).split("\n\n");
    for (const p of paragraphs) {
      const sentences = splitSentences(p);
      expect(sentences.length).toBeGreaterThanOrEqual(PARAGRAPH_MIN_SENTENCES);
      expect(sentences.length).toBeLessThanOrEqual(PARAGRAPH_MAX_SENTENCES);
    }
  });

  it("each sentence has 6-14 words and ends with a period", () => {
    const paragraphs = generateParagraphs(5).split("\n\n");
    for (const p of paragraphs) {
      expect(p.endsWith(".")).toBe(true);
      for (const s of splitSentences(p)) {
        const words = countWords(s);
        expect(words).toBeGreaterThanOrEqual(SENTENCE_MIN_WORDS);
        expect(words).toBeLessThanOrEqual(SENTENCE_MAX_WORDS);
      }
    }
  });

  it("each sentence's first word is capitalized", () => {
    const paragraphs = generateParagraphs(5).split("\n\n");
    for (const p of paragraphs) {
      for (const s of splitSentences(p)) {
        const first = s.split(" ")[0]!;
        expect(first.charAt(0)).toBe(first.charAt(0).toUpperCase());
      }
    }
  });

  it("two consecutive calls produce different output", () => {
    // Probabilistic: with a 70-word corpus + length variance across ~25 sentences
    // per batch, the chance of a collision is vanishingly small.
    const a = generateParagraphs(5);
    const b = generateParagraphs(5);
    expect(a).not.toBe(b);
  });
});
