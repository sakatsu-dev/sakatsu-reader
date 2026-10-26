import type { Term } from "../model";

/** A run of text, marked with a glossary id when it is a term worth offering. */
export type Segment = { text: string; id?: string };

/**
 * The shortest alias that may be matched inside a sentence.
 *
 * The glossary answers to codes as well as words — `n`, `vi`, `id`, `int`, `exp`, `past`,
 * `stem`. Those are exactly right on a badge and wrong in prose: "id" and "int" are not
 * English words at all, and "past", "stem" and "noun" are common enough that marking every
 * one would produce the clutter this feature exists to avoid. Five characters keeps the
 * jargon a reader might not remember — causative, predicate, transitive, conjunction — and
 * drops the rest.
 */
const SHORTEST = 5;

/** Aliases worth looking for in a sentence, longest first so the longest wins. */
function candidates(terms: Term[]): { alias: string; id: string }[] {
    return terms
        .flatMap((term) => term.aliases.map((alias) => ({ alias, id: term.id })))
        // ASCII words only. Every Japanese alias is skipped for free, which is also what
        // keeps a match out of the Japanese in an example sentence.
        .filter(({ alias }) => alias.length >= SHORTEST && /^[a-z][a-z-]*$/.test(alias))
        .sort((a, b) => b.alias.length - a.alias.length);
}

const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * Split `text` so the glossary terms in it can be offered.
 *
 * **Only the first mention of each term**, so a paragraph that says "passive" three times
 * is marked once. Whole words only: "particle" must not match inside "particles" unless
 * that plural is an alias in its own right, and nothing should match mid-word.
 */
export function mark(text: string, terms: Term[]): Segment[] {
    const wanted = candidates(terms);
    if (wanted.length === 0) return [{ text }];

    const pattern = new RegExp(
        `\\b(${wanted.map(({ alias }) => escape(alias)).join("|")})\\b`,
        "gi",
    );
    const byAlias = new Map(wanted.map(({ alias, id }) => [alias, id]));

    const out: Segment[] = [];
    const already = new Set<string>();
    let at = 0;

    for (const found of text.matchAll(pattern)) {
        const id = byAlias.get(found[0].toLowerCase());
        if (!id || already.has(id)) continue;
        already.add(id);

        const start = found.index ?? 0;
        if (start > at) out.push({ text: text.slice(at, start) });
        out.push({ text: found[0], id });
        at = start + found[0].length;
    }

    if (at < text.length) out.push({ text: text.slice(at) });
    return out.length > 0 ? out : [{ text }];
}
