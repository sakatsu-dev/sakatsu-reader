/**
 * What the reference and glossary pages keep between visits.
 *
 * Both are unmounted the moment you switch views, so anything they held in local state was
 * thrown away — and going to the reader *because of* a result is the commonest thing to do
 * next, which made it exactly the trip that lost the result. The glossary was worse: it
 * refetched several hundred terms every time it was opened.
 *
 * The same shape as `fetching.svelte.ts`: a class of `$state` fields exported as one
 * instance, so components that mount and unmount independently share it.
 *
 * **The answers live here; the questions live in the store.** A tab's query is written to
 * `settings.reference_tabs` and comes back on launch — see `Tab.pending` for what happens
 * then. Its results are not: they are derived data, they would bloat the settings file, and
 * they would be an answer built against whatever dictionaries were installed *last* time.
 *
 * What comes back is a *tab*, not a typed query: the search box is empty on launch. The two
 * used to be the same thing, because the box was the only place a search survived; the strip
 * is that place now, and a box arriving pre-filled only had to be cleared before anything
 * else could be looked up.
 */
import type {
  GrammarPattern,
  KanjiView,
  ParticleView,
  Term,
  WordHit,
} from "../model";

/** One search: a question and its answer. */
export class Tab {
  /** What is in the box. Bound to the input while this tab is in front. */
  query = $state("");
  /** The query the results below belong to; empty before the first search. */
  asked = $state("");
  searching = $state(false);
  error = $state("");
  /** In-flight and failed questions stay separate from the successful answer. */
  requested = $state("");
  failedQuery = $state("");

  words = $state<WordHit[]>([]);
  patterns = $state<GrammarPattern[]>([]);
  terms = $state<Term[]>([]);
  particles = $state<ParticleView[]>([]);
  kanji = $state<KanjiView | null>(null);

  /** Presentation state belongs to the tab just as its answer does. */
  facet = $state<"all" | "words" | "grammar" | "kanji" | "terms">("all");
  wordKinds = $state<string[]>([]);
  filtersOpen = $state(false);
  character = $state<string | null>(null);
  /** The sole inline detail currently mounted; `null` leaves the collection collapsed. */
  selectedKey = $state<string | null>(null);
  collectionScroll = $state(0);

  /** Which way the backend read the query, and what ordered the words. */
  japanese = $state(false);
  rankedBy = $state<string | null>(null);
  /** The query was English and no dictionary has a gloss index yet. */
  needsIndex = $state(false);
  /** More entries matched than came back — see `Found.capped` for why it is not a number. */
  capped = $state(false);

  /**
   * The question this tab was restored with and has not asked yet. Empty when nothing waits.
   *
   * Restoring six tabs must not fire six searches at launch, so a restored tab carries its
   * question and waits. The reference page runs the one in front on mount and each of the
   * others the first time it is brought forward, which spreads the cost over the moments
   * someone actually asked to see an answer.
   *
   * **The question itself, not a flag, because the box no longer holds it.** A restart used
   * to come back with the last search typed into the search box, from the days when the box
   * was the only place a search was kept. The tab strip keeps them now, so the box comes back
   * empty and ready — and that leaves a restored tab's question with nowhere else to live
   * until it runs. Which is where it belongs: `query` means *what is in the box*, and after a
   * restart nothing is.
   */
  pending = $state("");

  /**
   * Which search is the current one.
   *
   * Not `$state`: nothing renders it, and two searches in flight can land in either order.
   * It lives here rather than in the component so that leaving the page mid-search and
   * coming back still discards the older reply — the counter has to outlive the mount for
   * that to hold. Per tab, because two tabs can legitimately be searching at once.
   */
  generation = 0;

  constructor(query = "") {
    this.query = query;
  }

  /**
   * What the tab strip calls this tab: what it answered, what it is waiting to ask, or what
   * is being typed into it.
   *
   * `pending` sits in the middle because a restored tab has answered nothing and has an empty
   * box — without it the strip would come back after a restart as a row of blank tabs, and
   * `stored()` would drop every one of them on the next save.
   */
  get label(): string {
    return this.asked || this.pending || this.query.trim();
  }

  /**
   * Whether this tab stands for a search at all.
   *
   * Answered, running, or restored from the store and waiting its turn — all three are a
   * question this tab owns, and all three deserve a tab in the strip.
   *
   * Deliberately not `label`, which falls back to whatever is in the box. Keyed on that, the
   * strip would appear on the first keystroke of a fresh session and rename itself on every
   * one after — a tab is a search you made, not a search you are still typing.
   */
  get searched(): boolean {
    return this.asked !== "" || this.pending !== "" || this.searching;
  }

  /** Back to an empty tab, keeping whatever is in the box. */
  reset() {
    // Invalidate replies before clearing the view, including an initial search.
    this.generation += 1;
    this.searching = false;
    this.requested = "";
    this.failedQuery = "";
    this.filtersOpen = false;
    this.character = null;
    this.asked = "";
    this.words = [];
    this.patterns = [];
    this.terms = [];
    this.particles = [];
    this.kanji = null;
    this.facet = "all";
    this.wordKinds = [];
    this.selectedKey = null;
    this.collectionScroll = 0;
    this.error = "";
    this.needsIndex = false;
    this.rankedBy = null;
    this.capped = false;
    this.pending = "";
  }
}

/** The reference page's tabs, and which one is in front. */
class Search {
  /**
   * Left to right, and **never empty**.
   *
   * That invariant is the whole reason `current` can be a plain getter rather than
   * something every caller has to null-check: closing the last tab opens a fresh one in its
   * place, so there is always a tab to type into. A reference page with no tabs would have
   * no search box either.
   */
  tabs = $state<Tab[]>([new Tab()]);
  active = $state(0);

  /**
   * Tabs that were closed, most recent last — what `Ctrl+Shift+T` reaches back into.
   *
   * **Whole tabs, not queries.** Reopening one puts its results straight back rather than asking
   * the backend again, which is what the shortcut is for: it undoes a mistake, and a mistake that
   * takes two seconds to undo has already cost more than the tab was worth.
   *
   * In memory and not in `Settings`. The stack is a session convenience — a browser's is too —
   * and `stored()` already writes the open tabs; a second list of closed ones in the settings file
   * would be a thing to cap, clamp and migrate for no gain a reader would notice.
   */
  closed = $state<Tab[]>([]);

  /** How far back `Ctrl+Shift+T` can reach. Enough to undo a misfire, not a history. */
  static readonly REOPENABLE = 10;

  /** The tab in front. Total, by the invariant above. */
  get current(): Tab {
    return this.tabs[this.active] ?? this.tabs[0];
  }

  /** A new tab at the end, brought to the front. Returns it so the caller can focus it. */
  open(query = ""): Tab {
    const tab = new Tab(query);
    this.tabs.push(tab);
    this.active = this.tabs.length - 1;
    return tab;
  }

  /**
   * Close one, and land somewhere sensible.
   *
   * Closing a tab left of the one in front must move `active` down with it, or the page
   * silently switches to a different result — the browser behaviour this is modelled on
   * keeps you where you were.
   */
  close(index: number) {
    if (index < 0 || index >= this.tabs.length) return;
    this.remember(this.tabs[index]);
    this.tabs.splice(index, 1);
    if (this.tabs.length === 0) {
      this.tabs.push(new Tab());
      this.active = 0;
      return;
    }
    if (index < this.active) this.active -= 1;
    else if (index === this.active) this.active = Math.min(index, this.tabs.length - 1);
  }

  /**
   * Bring back the last tab closed, and the one before that, and so on.
   *
   * Appended at the end rather than at the index it came from. Putting it back where it was would
   * be defensible, but it would also renumber every tab to its right — and `Ctrl+1`…`Ctrl+9` mean
   * positions. Somewhere predictable beats somewhere faithful.
   */
  reopen(): Tab | null {
    const tab = this.closed.pop();
    if (!tab) return null;
    this.tabs.push(tab);
    this.active = this.tabs.length - 1;
    return tab;
  }

  /**
   * Remember a tab worth reopening.
   *
   * A tab with nothing in it is not worth a slot: closing a blank tab and pressing
   * `Ctrl+Shift+T` should reach past it to the last real search, exactly as a browser skips a
   * `New Tab` page.
   */
  private remember(tab: Tab) {
    if (!tab.label) return;
    this.closed.push(tab);
    if (this.closed.length > Search.REOPENABLE) this.closed.shift();
  }

  /** The next tab along, wrapping at both ends — which is what `Ctrl+Tab` does everywhere. */
  step(delta: number) {
    const count = this.tabs.length;
    if (count === 0) return;
    this.active = (((this.active + delta) % count) + count) % count;
  }

  /**
   * Go to a tab by position. **A negative index counts from the end**, so `jump(-1)` is the last
   * one — which is what `Ctrl+9` means.
   *
   * Out of range does nothing rather than clamping: `Ctrl+5` with three tabs open should feel
   * like a key that is not bound yet, not like a key that silently means something else.
   */
  jump(index: number) {
    const at = index < 0 ? this.tabs.length + index : index;
    if (at < 0 || at >= this.tabs.length) return;
    this.active = at;
  }

  /**
   * Close every tab at once, back to one empty one.
   *
   * The same invariant `close` keeps, and for the same reason: there is always a tab to type
   * into, so `current` can be a plain getter. "Clear all" therefore means one blank tab, not
   * none — and the page it leaves behind is the one a first run opens on.
   */
  clearAll() {
    // Every one of them, oldest first, so `Ctrl+Shift+T` walks back through the strip in the
    // order it stood. Clearing the lot is the easiest thing here to regret.
    for (const tab of this.tabs) this.remember(tab);
    this.tabs = [new Tab()];
    this.active = 0;
  }

  /**
   * What to write to the store: the queries, and which was in front.
   *
   * Tabs with nothing in them are dropped — an empty tab is a place to type, not a question
   * worth restoring — and `active` is remapped onto what survives, because dropping a tab to
   * the left of it would otherwise restore a different search than the one that was open.
   */
  stored(): { tabs: string[]; active: number } {
    const kept: string[] = [];
    let active = 0;
    this.tabs.forEach((tab, index) => {
      const label = tab.label;
      if (!label) return;
      if (index <= this.active) active = kept.length;
      kept.push(label);
    });
    return { tabs: kept, active };
  }

  /**
   * Put the stored queries back, as tabs that have not run yet.
   *
   * Rust clamps the index and caps the list before this ever sees them — see
   * `Settings::normalise` — so this does not re-check either. An empty list restores the
   * single empty tab, which is also what a fresh install gets.
   */
  restore(queries: string[], active: number) {
    // `new Tab()` and not `new Tab(query)`: the query goes to `pending`, which is the tab's
    // question, and the box stays empty. See `pending`.
    const tabs = queries.filter((query) => query.trim()).map((query) => {
      const tab = new Tab();
      tab.pending = query;
      return tab;
    });
    if (tabs.length === 0) return;
    this.tabs = tabs;
    this.active = Math.min(Math.max(active, 0), tabs.length - 1);
  }
}

/** The glossary page's list and its filter. */
class Glossary {
  terms = $state<Term[]>([]);
  error = $state("");
  loading = $state(false);
  /** What the filter box holds. */
  find = $state("");
  selectedId = $state<string | null>(null);
  collectionScroll = $state(0);
  /**
   * Whether the fetch has been made this session.
   *
   * Separate from `terms.length`, so a glossary that genuinely came back empty — or one
   * that failed — is not re-requested on every visit.
   */
  loaded = false;
}

export const search = new Search();
export const glossary = new Glossary();
