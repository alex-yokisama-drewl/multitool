const CORPUS = [
  "lorem",
  "ipsum",
  "dolor",
  "sit",
  "amet",
  "consectetur",
  "adipiscing",
  "elit",
  "sed",
  "do",
  "eiusmod",
  "tempor",
  "incididunt",
  "ut",
  "labore",
  "et",
  "dolore",
  "magna",
  "aliqua",
  "enim",
  "ad",
  "minim",
  "veniam",
  "quis",
  "nostrud",
  "exercitation",
  "ullamco",
  "laboris",
  "nisi",
  "aliquip",
  "ex",
  "ea",
  "commodo",
  "consequat",
  "duis",
  "aute",
  "irure",
  "in",
  "reprehenderit",
  "voluptate",
  "velit",
  "esse",
  "cillum",
  "fugiat",
  "nulla",
  "pariatur",
  "excepteur",
  "sint",
  "occaecat",
  "cupidatat",
  "non",
  "proident",
  "sunt",
  "culpa",
  "qui",
  "officia",
  "deserunt",
  "mollit",
  "anim",
  "id",
  "est",
  "laborum",
  "vitae",
  "natus",
  "error",
  "voluptatem",
  "accusantium",
  "doloremque",
  "laudantium",
  "totam",
  "rem",
  "aperiam",
] as const;

export const SENTENCE_MIN_WORDS = 6;
export const SENTENCE_MAX_WORDS = 14;
export const PARAGRAPH_MIN_SENTENCES = 4;
export const PARAGRAPH_MAX_SENTENCES = 7;

function randInt(min: number, max: number): number {
  return Math.floor(Math.random() * (max - min + 1)) + min;
}

function pickWord(): string {
  return CORPUS[Math.floor(Math.random() * CORPUS.length)]!;
}

function capitalize(word: string): string {
  return word.charAt(0).toUpperCase() + word.slice(1);
}

function sentence(): string {
  const length = randInt(SENTENCE_MIN_WORDS, SENTENCE_MAX_WORDS);
  const words = Array.from({ length }, pickWord);
  words[0] = capitalize(words[0]!);
  // Drop a comma after a random interior word for cadence, but not next to the
  // start or end — keeps the comma from looking like punctuation noise.
  if (length >= 8) {
    const commaAt = randInt(2, length - 3);
    words[commaAt] = `${words[commaAt]!},`;
  }
  return `${words.join(" ")}.`;
}

function paragraph(): string {
  const length = randInt(PARAGRAPH_MIN_SENTENCES, PARAGRAPH_MAX_SENTENCES);
  return Array.from({ length }, sentence).join(" ");
}

export function generateParagraphs(count: number): string {
  return Array.from({ length: count }, paragraph).join("\n\n");
}
