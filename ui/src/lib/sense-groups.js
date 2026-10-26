/**
 * @typedef {{ sense: { tags: string[] }, credit: string | null, component?: string | null }} GroupableSense
 */

/**
 * Build the two honest levels of classification in a dictionary sense list.
 *
 * A section is one source's consecutive senses. Tags common to the whole section sit above it;
 * after those are removed, senses with the same leading classification are collected into stable
 * groups. That distinction is what 有る needs: `godan irregular` applies to all six meanings,
 * `intransitive` only to 1–5, and `auxiliary verb` to 6. It also keeps a lone `interjection`
 * meaning from cutting one `particle` classification into two visual groups.
 *
 * This is a presentation grouping, not a semantic ranking: group order is the order each
 * classification first appears, and sense order inside a group remains the source order. The
 * leading remaining tag is deliberate. Dictionary and analyzer tag lists put the grammatical
 * classification before narrower qualifiers; later tags still stay on their individual rows.
 * A credit boundary always starts a new section so one dictionary can never appear to own another's
 * definitions. Visible senses and folded senses are passed separately by the caller, so a hidden
 * meaning can never change a heading outside its fold.
 *
 * @template {GroupableSense} T
 * @param {readonly T[]} senses
 * @param {number} [offset]
 * @param {boolean} [collectSeparated]
 * @returns {{
 *   items: T[],
 *   start: number,
 *   sharedTags: string[],
 *   credit: string | null,
 *   component: string | null,
 *   groups: { items: T[], start: number, sharedTags: string[] }[]
 * }[]}
 */
export function groupSenseSections(senses, offset = 0, collectSeparated = true) {
  const sections = [];
  let sectionStart = 0;

  while (sectionStart < senses.length) {
    const credit = senses[sectionStart].credit;
    const component = senses[sectionStart].component ?? null;
    let sectionEnd = sectionStart + 1;
    while (
      sectionEnd < senses.length &&
      senses[sectionEnd].credit === credit &&
      (senses[sectionEnd].component ?? null) === component
    ) {
      sectionEnd += 1;
    }

    const sectionItems = senses.slice(sectionStart, sectionEnd);
    const sharedTags =
      sectionItems.length < 2
        ? []
        : sectionItems[0].sense.tags.filter((code) =>
            sectionItems.every((listed) => listed.sense.tags.includes(code)),
          );

    /** @param {T} listed */
    const remaining = (listed) =>
      listed.sense.tags.filter((code) => !sharedTags.includes(code));
    /** @type {{ key: string | null, items: T[], sharedTags: string[] }[]} */
    const buckets = [];
    /** @type {Map<string | null, { key: string | null, items: T[], sharedTags: string[] }>} */
    const byKey = new Map();

    for (const listed of sectionItems) {
      const tags = remaining(listed);
      // Untagged meanings may share one uninterrupted list. Classified one-offs need unique
      // buckets or two unrelated single senses would be moved together merely because neither
      // classification repeats.
      const key = tags[0] ?? null;
      const last = buckets[buckets.length - 1];
      /** @type {{ key: string | null, items: T[], sharedTags: string[] } | undefined} */
      let bucket = collectSeparated
        ? byKey.get(key)
        : last?.key === key
          ? last
          : undefined;
      if (!bucket) {
        bucket = { key, items: [], sharedTags: tags };
        buckets.push(bucket);
        if (collectSeparated) byKey.set(key, bucket);
      }
      bucket.items.push(listed);
      bucket.sharedTags = bucket.sharedTags.filter((code) => tags.includes(code));
    }

    let nextStart = offset + sectionStart;
    const groups = buckets.map((bucket) => {
      // A heading above one meaning removes no repetition.
      const group = {
        items: bucket.items,
        start: nextStart,
        sharedTags: bucket.items.length < 2 ? [] : bucket.sharedTags,
      };
      nextStart += bucket.items.length;
      return group;
    });
    const orderedItems = groups.flatMap((group) => group.items);

    sections.push({
      items: orderedItems,
      start: offset + sectionStart,
      sharedTags,
      credit,
      component,
      groups,
    });
    sectionStart = sectionEnd;
  }

  return sections;
}

/**
 * Apply the same presentation order to a bare sense list.
 *
 * Analysis passes its `WordView` to Anki as well as to `WordBody`. Keeping this small adapter next
 * to the grouping rule prevents a card's first meanings from disagreeing with the meanings the
 * learner just saw at the top of the vocabulary pane.
 *
 * @template {{ tags: string[] }} T
 * @param {readonly T[]} senses
 * @returns {T[]}
 */
export function orderSensesForDisplay(senses) {
  const listed = senses.map((sense) => ({ sense, credit: null }));
  return groupSenseSections(listed).flatMap((section) =>
    section.items.map((item) => item.sense),
  );
}
