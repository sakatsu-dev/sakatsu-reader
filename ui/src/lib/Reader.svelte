<script lang="ts">
  import type { Match, PassageAlign, Ruby, Token, WordStatus } from "../model";
  import { isWord } from "./japanese";
  import { lineUnits, passageLines } from "./passage.js";

  let {
    text = null,
    tokens,
    patterns,
    statuses,
    ruby,
    selected,
    studied,
    furigana = true,
    keepLineBreaks = false,
    passageAlign = "left",
    markReadingGuesses = false,
    tabNavigation = false,
    focusRequest = 0,
    onselect,
    onknow,
    onstudy,
  }: {
    /**
     * The passage as analysed, so the reader can break where it breaks.
     *
     * Optional and `null`-tolerant: without it every token lands on one flowing line, which is
     * the right fallback — the alternative is guessing at breaks, which is the bug this replaced.
     */
    text?: string | null;
    tokens: Token[];
    patterns: Match[];
    statuses: WordStatus[];
    /** Parallel to `tokens` — how each word is set, from `dict::furigana`. */
    ruby: Ruby[][];
    selected: number | null;
    /**
     * The words ticked into the comparison set, which is a different thing from `selected`.
     *
     * `selected` is the one word the dictionary and grammar panels are describing — always
     * exactly one. This is the sentence-bound set being studied together, built by Ctrl+click;
     * adding a word in another sentence starts the comparison there. A word can be in both the
     * comparison and the panel selection at once. They therefore need visibly different
     * treatments: confusing "the word I am reading about" with "the words I am comparing" would
     * make the strip below look like it was describing the panel beside it.
     */
    studied: Set<number>;
    furigana?: boolean;
    /** Lay the passage out with its own line breaks rather than as one paragraph. */
    keepLineBreaks?: boolean;
    /**
     * Where each line starts. A `text-align` keyword, set on the reader so every line inherits it.
     * Defaults to `left`, following `Settings::default`.
     */
    passageAlign?: PassageAlign;
    /// Whether to draw the dotted rule under a reading the app is not sure of.
    ///
    /// Defaults to `false`, following `Settings::default` — the setting is off, so a caller that
    /// forgets this should render what the app actually ships rather than something no reader
    /// asked for. The earlier default was `true` on the argument that losing a caveat silently is
    /// the worse failure; that held while the setting was on and stopped holding when it went off.
    markReadingGuesses?: boolean;
    /** 縦書き: down the page, columns advancing leftward. */
    /** Conventional keyboard mode: expose one roving Tab stop in the passage. */
    tabNavigation?: boolean;
    /** Incremented only when arrow navigation should move DOM focus with selection. */
    focusRequest?: number;
    onselect: (index: number) => void;
    /** Pointer shortcut for Known; the status controls remain the keyboard route. */
    onknow: (index: number) => void;
    /** Ctrl/Cmd+click: tick a word in or out of the comparison set. */
    onstudy: (index: number) => void;
  } = $props();

  // Tokens covered by a matched grammar pattern get a marker, so a particle the
  // grammar view can explain is visible in the text itself.
  const inPattern = $derived.by(() => {
    const covered = new Set<number>();
    for (const match of patterns) {
      for (let i = match.first_token; i < match.last_token_end; i += 1) covered.add(i);
    }
    return covered;
  });

  /// Furigana is set per character where the reading can be split across them, and over
  /// the whole word where it cannot — `dict::furigana` decides which, and answers
  /// whole-word for 熟字訓 and whenever the kanji table is not installed.
  ///
  /// The runs arrive with the analysis rather than being asked for per word: the reader
  /// draws every token at once, and a round trip each would be hundreds for a page.
  function runs(index: number, token: Token): Ruby[] {
    // Known is the reader's settled state: keep the stored ruby for later, but do not
    // ask the eye to read a pronunciation it has already learned. Seen still gets help.
    if (!furigana || statuses[index] === "known") return [{ text: token.surface, reading: null }];
    return ruby[index] ?? [{ text: token.surface, reading: null }];
  }

  /// Whether anything on this page carries a reading.
  ///
  /// Ruby needs headroom above the line, and a line height set for it is paid by every
  /// line whether or not it has any — CSS cannot vary that per line. A passage with no
  /// furigana anywhere can therefore be set normally, which covers a kana passage and any
  /// passage read with furigana turned off.
  const anyFurigana = $derived(
    furigana && ruby.some((word, index) =>
      tokens[index] && isWord(tokens[index]) && statuses[index] !== "known" &&
      word.some((run) => run.reading !== null)),
  );

  /// Lines, broken **where the passage breaks them** — and nowhere else.
  ///
  /// This used to start a new line after every `。！？`, so a long capture would not arrive as
  /// one unbroken wall. It was too strong a rule: `提出締め切り！つぎのステップへ` is one line of
  /// input and came back as two, with the width of the window empty to the right of both. Text
  /// that was never broken should not look broken, and a reader comparing the passage against
  /// what they pasted should find the same shape.
  ///
  /// The wall it was guarding against is still handled, because a capture that has lines *has
  /// newlines* — OCR of a manga panel or a game box returns them, and those are honoured here.
  /// A single long paragraph now wraps the way a paragraph should.
  ///
  /// Falls back to one line when the analyzer could not ground a token (`start` is null), since
  /// without offsets there is nothing to read the breaks from and inventing them is what this
  /// change is undoing.
  ///
  /// Indices are carried through rather than recomputed: the arrow keys, `statuses`, `ruby` and
  /// `data-token` all index the flat token array, and regrouping is the obvious way to break
  /// every one of them at once.
  const lines = $derived(
    passageLines(text, tokens, keepLineBreaks).map((line: number[]) =>
      line.map((index: number) => ({ index, token: tokens[index] })),
    ),
  );
  const firstWord = $derived(
    tokens.findIndex((token) => isWord(token)),
  );

  /// Keep the selected word on screen.
  ///
  /// The arrow keys walk the text faster than the eye tracks it, and a selection that
  /// has moved below the fold looks exactly like a selection that did not move at all.
  ///
  /// `nearest` rather than centring, because scrolling on every step — including the
  /// steps where the word was already comfortably in view — makes the passage twitch
  /// under the reader. And no smooth behaviour: held arrow keys repeat faster than a
  /// smooth scroll finishes, so it would fall behind and animate to somewhere the
  /// selection had already left. Both axes are asked for, which costs nothing and keeps this
  /// working if the passage ever scrolls sideways.
  let reader = $state<HTMLDivElement | undefined>();

  $effect(() => {
    if (selected === null || !reader) return;
    reader
      .querySelector<HTMLElement>(`[data-token="${selected}"]`)
      ?.scrollIntoView({ block: "nearest", inline: "nearest" });
  });

  $effect(() => {
    if (!tabNavigation || focusRequest <= 0 || selected === null || !reader) return;
    void focusRequest;
    reader.querySelector<HTMLElement>(`[data-token="${selected}"]`)?.focus({ preventScroll: true });
  });
</script>

<div
  class="reader"
  class:has-ruby={anyFurigana}
  style:text-align={passageAlign}
  lang="ja"
  bind:this={reader}
>
  {#each lines as line}
    <p class="line">
      <!-- **One unbreakable run per unit**, so 。 never begins a line and 「 never ends one. Each
           word is a button, an atomic inline the line breaker will break beside, so the ordinary
           rule never applied — see `lineUnits`. -->
      {#each lineUnits(line.map((entry: { index: number }) => entry.index), tokens, isWord) as unit}
      <span class="unit">
      {#each unit.map((index: number) => ({ index, token: tokens[index] })) as { index, token }}
        {#if isWord(token)}
          <button
            data-token={index}
            data-reader-token
            class="token status-{statuses[index] ?? 'new'}"
            class:selected={selected === index}
            class:studied={studied.has(index)}
            class:pattern={inPattern.has(index)}
            class:unverified={markReadingGuesses && token.reading_unverified}
            class:ambiguous={markReadingGuesses && token.reading_ambiguous}
            tabindex={tabNavigation && (selected === index || (selected === null && index === firstWord)) ? 0 : -1}
            aria-current={selected === index ? "true" : undefined}
            onclick={(event) => {
              // **Ctrl/Cmd strikes the word — and now selects it too.** This once did the
              // opposite, on the reasoning that moving the lookup would throw away the panel the
              // learner was reading. That held while the English column printed the authored
              // sense and pack explanation for each struck word; it stopped holding when those
              // moved to Grammar, which describes whatever is *selected*. Striking without
              // selecting would now leave the cited answer for that word off screen entirely.
              // `App.svelte`'s `toggleStudied` owns the decision — see the comment there.
              // `metaKey` so the gesture is the same on a Mac, where Cmd is what selects files.
              if (event.ctrlKey || event.metaKey) {
                event.preventDefault();
                onstudy(index);
                return;
              }
              onselect(index);
            }}
            oncontextmenu={(event) => {
              // A pointer shortcut, not the only route: the vocabulary panel's status
              // buttons remain keyboard accessible. Do not select here — preserving the
              // open lookup is what makes clearing an already-known word one gesture.
              event.preventDefault();
              onknow(index);
            }}
            title={token.dict_form && token.dict_form !== token.surface
              ? `${token.dict_form}`
              : undefined}
          >
            {#each runs(index, token) as run}
              {#if run.reading}
                <ruby>{run.text}<rt>{run.reading}</rt></ruby>
              {:else}{run.text}{/if}
            {/each}
          </button>
        {:else}
          <span class="punctuation">{token.surface}</span>
        {/if}
      {/each}
      </span>
      {/each}
    </p>
  {/each}
</div>

<style>
  .reader {
    /* Scaled apart from the interface, because reading a passage and reading the buttons
       around it happen at different distances. The scale carries a `, 1` fallback for the
       capture frame, which shares this bundle and never runs the code that sets it — see
       `theme.css`, which is where these live now because the pane outside needs them too. */
    font-size: var(--reader-font);
    line-height: 1.75;
    padding: var(--gap-xs) 0 var(--gap);
    overflow-wrap: anywhere;
    /* **Never smaller than one line, whatever the pane does.**
     *
     * The reader used to have no height of its own at all, so a short window or a dragged
     * divider shrank the scrollport around it and the passage was simply cut — furigana
     * sliced off the top, the status rule off the bottom, and a scrollbar offered as the
     * remedy. A floor is the honest shape: the reader states what one line costs and the
     * split refuses to go under it, rather than every caller guessing.
     *
     * `--reader-line` is the ruby leading rather than the 1.75 below, because it has to fit
     * the taller of the two cases — and it includes this padding, since everything is
     * border-box. */
    min-height: var(--reader-line);
  }
  /* Ruby needs headroom or furigana collides with the line above — and only then. A
     passage carrying no readings was being spaced as though it did. */
  .reader.has-ruby {
    line-height: var(--reader-leading);
  }


  /* One line **of the passage**, wrapping as it needs to. There is one of these per newline in
     the text, not one per sentence — see `lines` for why that changed. */
  .line {
    margin: 0;
  }
  /* Lines sit apart down the page, which is the direction the text runs. */
  .line + .line {
    margin-top: var(--gap-xs);
  }

  .token {
    font: inherit;
    background: none;
    border: 0;
    border-bottom: 2px solid transparent;
    /* **Square along the bottom, and that is what makes the rule below an underline.**
     *
     * It was `var(--radius-sm)` on all four corners, which is 4px against 2px of horizontal
     * padding — the curve is wider than the space it has to turn in, so the rule started bending
     * *underneath the glyphs* rather than beyond them. Every word read as sitting in a shallow
     * tile instead of being underlined, and no thickness would have fixed it.
     *
     * The top corners keep the radius: they belong to the hover fill and the selected wash, which
     * should read as a highlight rather than a block. Only the edge the rule runs along has to be
     * straight. */
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
    /* **The token's box hugs its own glyphs; the paragraph keeps the tall line.**
     *
     * `.reader.has-ruby` sets `line-height: 2.5` so furigana clears the line above. Half-leading
     * is symmetric, though, so it also put three quarters of an em of nothing *below* every word —
     * and the rule sits at the bottom of the box, which left it drifting 25px under the text it
     * was supposed to underline.
     *
     * Overriding the line-height here shrinks only this box. The `<p>`'s strut is still 2.5em, so
     * every line box keeps its full height and the lines stay exactly as far apart as they were;
     * the token simply stops claiming leading it was not using. The ruby annotation is positioned
     * against the base text rather than against this box, so the furigana does not move either —
     * it just overflows upward into leading the paragraph is still reserving.
     *
     * The offset is `padding-bottom` rather than more line-height because it is the one number
     * worth being able to read: it is how far the rule sits under the word, and nothing else
     * depends on it. */
    line-height: 1.15;
    /* Deep enough for two marks, not one. The grammar strip is painted at the foot of this
       padding and the status rule sits just under it, so the padding is the only place the gap
       between them can come from — at `0.1em` they touched and read as a single thick line with
       a speckled edge. See `.token.pattern`. */
    padding: 0 2px 0.2em;
    color: var(--text);
    /* `background-color`, not `background` — the grammar marker below is a background
       image, and the shorthand would wipe it on hover. */
    transition: background-color var(--anim) var(--ease);
  }
  .token:hover {
    background-color: var(--surface-hover);
  }

  /*
   * Status. The reward for progress is a page that stops asking for attention, so
   * only the unfinished states are marked at all:
   *
   *   new       a blue rule — worth a look
   *   seen      a yellow rule — met before, not yet settled
   *   known     nothing whatsoever
   *   ignored   faded out of the way
   *
   * Blue and yellow are the first two steps of the status scale in theme.css, and the
   * chips in the vocabulary panel use the same two — the control and the text have to
   * agree or the colour means nothing. The scale's other two ends deliberately do not
   * appear here: green on every known word would undo the point below, and red on an
   * ignored one would shout at the thing you asked to be quiet.
   *
   * New is a rule and not a wash because *every* word starts new. A wash meant to pick
   * a few words out of a page instead filled the page, and since the token is an
   * inline-block it filled the furigana's headroom too — so a first read arrived as a
   * wall of purple slabs with the words inside them. The rules sit at the box edge,
   * where they line up across a phrase instead of stacking into blocks.
   *
   * **The corner radius was quietly undoing that argument.** Rounded on all four corners, each
   * rule curved up at both ends and every word came out looking like the bottom of a tile — the
   * slab this was chosen to avoid, drawn in outline. Lining up across a phrase only means
   * something if the rules are straight. See `.token` above for the arithmetic.
   */
  .token.status-new {
    border-bottom-color: var(--blue-edge);
  }
  .token.status-seen {
    border-bottom-color: var(--yellow);
  }
  .token.status-ignored {
    color: var(--text-faint);
  }

  /* A grammar point the engine can explain.
   *
   * Its own channel, because status owns the bottom border and a word is very often
   * both new and part of a pattern — one border cannot say both, and status is declared
   * later, so the grammar marker would simply never be seen. This is a dashed strip
   * painted at the foot of the padding box, which puts it just above the status rule
   * rather than in place of it.
   *
   * **Lifted 3px off that rule, and the gap is the whole of why it is legible.** At `100%` it sat
   * flush on top of the status border, and two marks with no ground between them are one mark: a
   * 2px coloured rule with a grey fringe, which read as the rule being slightly out of focus
   * rather than as a dotted line meaning something. It had that ground for free while the token
   * box was three quarters of an em deep; once the rule came up under the word it had to be
   * asked for. `--text-dim` rather than `--text-faint` for the same reason — it is next to a
   * saturated line now and has to hold its own beside it. */
  .token.pattern {
    background-image: linear-gradient(90deg, var(--text-dim) 50%, transparent 50%);
    background-size: 5px 1px;
    background-repeat: repeat-x;
    background-position: 0 calc(100% - 3px);
    background-clip: padding-box;
  }

  /* **A reading the app is not sure of.**
   *
   * Two causes, one mark. `unverified` means *nothing* vouched for this reading — assembled
   * character by character because no name dictionary could rejoin the compound, which is 善福寺川
   * set as ぜん・ふく・てら・かわ. `ambiguous` means the opposite: several sources vouched for
   * *different* readings and one was picked, which `core/tests/heteronym_readings.rs` measures at
   * 69.2% correct. A learner does not need to know which of those happened. They need to know not
   * to trust the kana, and at `0.4em` a second underline pattern is a distinction nobody can see.
   *
   * **On the `<rt>` only, and that is the whole idea.** The word itself is not in doubt — it is on
   * the page, it is what it is — so marking the base text would caveat the wrong thing.
   *
   * **This used to say `color: var(--text-faint)`, which is what the base `rt` rule below already
   * sets.** It rendered identically to a certain reading for as long as it shipped, under a comment
   * arguing carefully for a distinction that was never drawn. The unused-selector gate cannot catch
   * that: the selector matches, it just changes nothing. `traps.md` records it.
   *
   * A decoration rather than a colour, because there is nowhere left to go on colour — `--text-faint`
   * is the faintest step the palette has and the base `rt` is already wearing it. The decoration is
   * `--text-dim` so the mark is a shade more present than the kana it qualifies; at this size a
   * dotted rule in the text's own colour disappears.
   *
   * Not on the token's bottom edge, where `.token.pattern` and the status rule already live — this
   * sits above the base text, against the ruby, and touches neither. */
  .token.unverified rt,
  .token.ambiguous rt {
    text-decoration: underline dotted var(--text-dim);
    text-underline-offset: 0.16em;
  }

  /* Selection outranks status: it is where you are, not what you know.
   *
   * A wash with a firm edge rather than a filled block of accent. Filled, it was the
   * heaviest thing on the page by a distance — the same weight the status wash was
   * rejected for above, applied to the one word you are trying to read. The word keeps
   * its own colour now, and its furigana keeps the ground it had everywhere else. */
  .token.selected {
    background-color: var(--selection-wash);
    border-bottom-color: var(--selection);
    box-shadow: inset 0 0 0 1px var(--selection-edge);
  }

  /* The comparison set: struck through and faded, because that is what is being *done* to these
     words — the strip below translates the sentence with them taken out, and the reader should
     show the sentence the model is about to be asked about.

     It rhymes with the strip, where the English the removal took is struck the same way. Reading
     down the page, the struck Japanese and the struck English are the same gesture.

     Not a fill: `.selected` owns that, and a word is very often both — the one being looked up and
     one of the ones being compared. Not an underline either, which is where it started: the bottom
     edge already carries status (`border-bottom-color`) and the grammar marker
     (`.token.pattern`'s dashed strip), so a third mark down there was three claims stacked in one
     place. A line through the middle is the only free channel left. */
  .token.studied {
    text-decoration: line-through;
    text-decoration-color: var(--accent);
    text-decoration-thickness: 2px;
    color: var(--text-faint);
  }

  .punctuation {
    color: var(--text-dim);
  }

  /* A word with the marks that may not be parted from it. No break *between* its pieces; a word
     too long for the line can still wrap inside itself, as it always could. */
  .unit {
    white-space: nowrap;
  }
  .unit > .token {
    white-space: normal;
  }

  rt {
    font-size: 0.4em;
    color: var(--text-faint);
    font-family: var(--font-jp);
    user-select: none;
  }
</style>
