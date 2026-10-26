<script lang="ts">
  // The badges a sense wears, written out — `noun`, `intransitive`, `godan verb`.
  //
  // Extracted from `SenseRow` when a second place needed them: `SenseGroupHeading` hoists the
  // tags a consecutive run of senses shares, so 有る says `intransitive` once for meanings
  // one through five and starts a new `auxiliary verb` row at six. Two places drawing the same
  // chips is exactly the situation `traps.md` records — scoped styles do not travel with copied
  // markup, and these badges have already lost both their styling and their behaviour that way.
  //
  // **No wrapper element, on purpose.** This renders the chips and nothing around them, so each
  // caller supplies its own container and lays it out for itself: `SenseRow`'s `.tags` is
  // placed in a grid, `SenseGroupHeading` supplies the umbrella row. A wrapper here would need
  // its layout set here too, and neither caller could reach it — the same scoping wall the
  // extraction is meant to get around.
  import type { TagExplanation } from "../model";
  import TermButton from "./Term.svelte";

  let {
    tags,
    explained,
    opened,
    ontoggle,
    keyFor,
  }: {
    tags: string[];
    /** What each tag means, for the tags drawn here. Missing is normal. */
    explained: Record<string, TagExplanation>;
    /** Which note is open, across every row the caller draws. */
    opened: string | null;
    ontoggle: (key: string) => void;
    /** Names this chip's note so two rows carrying `noun` do not open together. */
    keyFor: (code: string) => string;
  } = $props();

  /// What a badge says, written out rather than abbreviated.
  ///
  /// The codes are the dictionary's shorthand, not the reader's: `n` and `adj-i` are things
  /// you have to already know to read, and the row has a line of its own with room to spare.
  /// Three sources, in the order they are worth reading:
  ///
  /// - the glossary's own name for the term — `n` is a *noun*, `v1` an *ichidan verb*;
  /// - failing that the dictionary's note, which is the only name a code like `v5m` has;
  /// - failing both the code itself, because there is nothing else to show. Those keep the
  ///   monospace chip, since then it really is a code.
  ///
  /// `label === code` is how `explain_tags` reports having found nothing but a bank note —
  /// its own comment puts it plainly: "a bank note has no name of its own beyond the code
  /// itself". So that comparison is reading a documented answer, not guessing at one.
  ///
  /// **Lowercased, because the two sources disagree about case.** The glossary capitalises
  /// its labels for use as a note's heading ("Noun — names a thing…") while a dictionary
  /// note is written as prose ("auxiliary verb"), and a row carrying both reads as two kinds
  /// of object. Lower is also how every dictionary prints a part of speech.
  function written(code: string): string {
    const found = explained[code];
    if (!found) return code;
    return (found.label === code ? found.detail : found.label).toLowerCase();
  }
</script>

{#each tags as code}
  {#if explained[code]}
    <TermButton variant="badge" open={opened === keyFor(code)} onclick={() => ontoggle(keyFor(code))}>
      {written(code)}
    </TermButton>
  {:else}
    <!-- Nothing to say about it, so nothing offered. The dotted underline means "there is
         more here", and a chip without one is a label.
         It keeps the monospace face the written-out chips have dropped, and here that is the
         point: this one really is a code, and nothing in the app can say what it stands for. -->
    <span class="pill">{code}</span>
  {/if}
{/each}
