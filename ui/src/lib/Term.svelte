<script lang="ts">
  // A grammatical term you can ask about, and which says nothing until you do.
  //
  // Deliberately a click and not a hover. These are words most readers know most of the
  // time, so an explanation that fires whenever the pointer crosses one is an interruption
  // rather than a help — and the same glossary answers from the reference page for anyone
  // who would rather go and look.
  //
  // Only the control. `TermNote.svelte` renders the explanation, and whoever owns the two
  // decides where the note goes: a badge lives in an inline row of other badges, so a note
  // rendered beside it would sit in that row instead of under the definition it belongs to.

  let {
    open = false,
    variant = "text",
    onclick,
    children,
  }: {
    open?: boolean;
    /** `badge` for a code on a sense, `text` for a word in prose or a heading. */
    variant?: "badge" | "text";
    onclick: () => void;
    children: import("svelte").Snippet;
  } = $props();
</script>

<button class="term {variant}" class:open {onclick} aria-expanded={open}>
  {@render children()}
</button>

<style>
  .term {
    font: inherit;
    color: inherit;
    background: none;
    border: 0;
    padding: 0;
    cursor: help;
    /* Quiet until wanted: a dotted underline says "there is more here" without turning
       every other word into a link. */
    text-decoration: underline dotted var(--border-strong);
    text-underline-offset: 0.2em;
  }
  .term:hover,
  .term.open {
    color: var(--accent);
    text-decoration-color: var(--accent);
  }

  /* The chip, written out rather than taken from `.pill` in theme.css.
     *
     * Not drift, and I tried it the other way first. `.term` above is a `<button>` and has to
     * reset one — `font: inherit`, `color: inherit`, `background: none`, `padding: 0` — and
     * scoped, that rule is `(0,2,0)` against the global `.pill`'s `(0,1,0)`. The shared chip
     * loses every declaration it has, and nothing in the build says so. Any rule here must
     * outrank those resets, which means it has to be here. See traps.md.
     *
     * Keep the two in step by hand if the chip changes.
     *
     * **No mono face, and its absence is doing work.** These badges say "noun" and "godan
     * verb ending in む" now, not `n` and `v5m`, and monospace is what this app uses to mean
     * "this is a dictionary's own abbreviation" — `SourceBadge` drops it for exactly that
     * reason and says so. A word set in mono reads as a code somebody spelled out. The face
     * is inherited from `.term`'s `font: inherit`, so removing the declaration is the whole
     * change; `.pill` in `SenseRow` keeps mono, because that branch is a bare code. */
  .term.badge {
    font-size: 0.66rem;
    padding: 0;
    /* **A label, not a pill.** Every sense wears one and a list of eleven meanings wore eleven
       grey lozenges, which read as eleven objects rather than as what a sense *is*. Uppercase and
       letterspaced is how this app already writes a label — `.section`, the kind badge, the pane
       names — so this joins that family instead of inventing a sixth chip shape.
       **The colour is inherited, not passed.** `--kind` is set on the sense row and on the group
       heading, and a custom property crosses the component boundary a scoped selector cannot; the
       fallback is what an unrecognised classification gets. See `word-kinds.js`. */
    background: none;
    color: var(--kind, var(--text-faint));
    font-family: var(--font-display);
    font-weight: 600;
    letter-spacing: 0.07em;
    text-transform: uppercase;
    /* The dotted underline is the whole affordance now that the lozenge has gone — it is what
       says an explanation is a click away, and it was already doing that job under the chip. */
    text-decoration-color: color-mix(in oklab, var(--kind, var(--text-faint)) 55%, transparent);
  }

  /* Open, the label brightens rather than changing hue: which classification it is has not
     changed, only whether its note is showing. Labels stay at interface size while the definitions
     scale — they are things you glance at, and growing them would cost the definitions their
     room. */
  .term.badge.open,
  .term.badge:hover {
    color: var(--kind, var(--text));
    text-decoration-color: currentColor;
    filter: brightness(1.25);
  }
</style>
