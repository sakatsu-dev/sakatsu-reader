<script lang="ts">
  import { bindingLabel, isBindableButton, type Binding, type MouseButton } from "../model";

  // Choosing what fires an action, by pressing it rather than typing its name.
  //
  // A text field would mean knowing that the plugin writes it "CmdOrCtrl+Shift+Q" and
  // that "Ctl" is not a modifier. Recording the next input is how every game does this,
  // and it cannot produce a chord the user did not actually press.
  //
  // **Any key can be bound**, including a bare one with no modifier. A bare ` is a perfectly
  // good binding for someone whose hands are on a manga rather than a keyboard, and refusing
  // it because it *could* be inconvenient is not a call this component gets to make. What
  // keeps that safe is the mouse hook never swallowing an event: a bound button goes on
  // doing whatever it normally does.
  //
  // **Left and right click are the exception, and for that same reason.** Not swallowing is
  // exactly what makes those two unusable: a left click selects a word and a right click marks
  // one Known, so either would fire on top of a reading gesture, with no click left over to
  // tell the two apart. Middle, Mouse 4 and Mouse 5 carry nothing in the reader.
  // `MouseButton::bindable` in Rust is the authority and `rebind` refuses there too; this
  // copy of the rule is here so the answer needs no round trip.
  //
  // Cancelling is a button rather than Escape, because Escape is bindable too. That is
  // also why whoever holds this component has to know when it is listening: the settings
  // drawer closes on Escape, and while a binding is being recorded Escape belongs here.

  let {
    label,
    bind,
    onchange,
    onrecording,
    clearable = false,
  }: {
    label: string;
    /** `null` where the binding may be absent and is — the switch hotkey's default. */
    bind: Binding | null;
    /** Called with `null` to clear a binding that may be absent. */
    onchange: (bind: Binding | null) => Promise<string>;
    /** Called whenever this starts or stops listening for the next input. */
    onrecording?: (active: boolean) => void;
    /** Whether "no binding at all" is a state this one may be put into. */
    clearable?: boolean;
  } = $props();

  let recording = $state(false);
  let error = $state("");

  // Reported from an effect rather than from `start`, `commit` and Cancel separately —
  // three call sites is three chances for one of them to stop reporting.
  // A block body, not an expression: an effect's return value is taken as its cleanup
  // function, and reporting through a callback that happens to return something would
  // hand Svelte a boolean where it expects a function.
  $effect(() => {
    onrecording?.(recording);
  });

  const MODIFIERS = ["Control", "Shift", "Alt", "Meta"];

  async function commit(next: Binding | null) {
    recording = false;
    error = await onchange(next);
  }

  function key(event: KeyboardEvent) {
    if (!recording) return;
    event.preventDefault();
    event.stopPropagation();

    // A modifier alone is not a binding — wait for the key it modifies. Held on its own
    // it is how a chord starts, not a chord.
    if (MODIFIERS.includes(event.key)) return;

    const parts: string[] = [];
    if (event.ctrlKey) parts.push("CmdOrCtrl");
    if (event.shiftKey) parts.push("Shift");
    if (event.altKey) parts.push("Alt");
    if (event.metaKey) parts.push("Super");

    // `code` rather than `key`: it is the physical key, so it is unaffected by which
    // modifiers are down or which layout is active, and its names — Backquote, KeyQ,
    // Digit1, F1 — are exactly the ones the accelerator parser accepts.
    parts.push(event.code);
    commit({ kind: "key", chord: parts.join("+") });
  }

  function mouse(event: MouseEvent) {
    if (!recording) return;
    // The Cancel button is the way out, so a click on it is not a binding.
    if ((event.target as HTMLElement)?.closest(".cancel")) return;

    const button: MouseButton | null =
      (["left", "middle", "right", "x1", "x2"] as const)[event.button] ?? null;
    if (!button) return;

    event.preventDefault();
    event.stopPropagation();

    // Still listening afterwards, deliberately. The refused gesture is one somebody just made
    // on purpose, and dropping out of recording would make them press Change again to try the
    // button they actually wanted.
    if (!isBindableButton(button)) {
      error =
        `${bindingLabel({ kind: "mouse", button })} is how the reader selects and marks words. ` +
        "Try the middle button, Mouse 4 or Mouse 5.";
      return;
    }

    commit({ kind: "mouse", button });
  }

  function start() {
    error = "";
    recording = true;
  }

  /// A key bound on its own reaches no other application.
  ///
  /// The comment at the top of this file says a bare ` is a fine binding, and that what keeps
  /// it safe is the mouse hook never swallowing an event. That is true of the **mouse**: a
  /// bound button goes on doing whatever it normally did. It is not true of a key. A chord
  /// registered as a global shortcut is routed here and delivered nowhere else, so a bare `
  /// stops typing in every application on the machine — including this one's own search box —
  /// for as long as the binding is live.
  ///
  /// Still not refused. It is a real choice for someone whose hands are on a manga rather than
  /// a keyboard, and this component does not get to overrule that. It does get to say so,
  /// which is the part that was missing: nobody finds this out until they try to type one.
  const bare = $derived(
    bind?.kind === "key" && !bind.chord.includes("+") ? bind.chord : null,
  );
</script>

<svelte:window onkeydown={key} onmousedown={mouse} onauxclick={mouse} oncontextmenu={mouse} />

<div class="bind">
  <span class="what">{label}</span>
  <button class="current" class:recording class:unset={!bind && !recording} onclick={start}>
    {recording ? "press anything…" : bind ? bindingLabel(bind) : "not set"}
  </button>
  {#if recording}
    <button class="cancel" onclick={() => (recording = false)}>Cancel</button>
  {:else if bind && clearable}
    <!-- Only where absence is a legitimate state. The capture and show/hide bindings are
         always set to something, and a Clear on those would offer a state they cannot be
         in. -->
    <button class="cancel" onclick={() => commit(null)}>Clear</button>
  {/if}
  {#if error}<span class="error">{error}</span>{/if}
</div>
{#if bare}
  <p class="bare-note">
    <strong>{bare}</strong> is bound on its own, so no other application will receive it while
    this binding is live — including this window's own text boxes. Adding a modifier
    (<strong>Ctrl</strong> or <strong>Alt</strong>) leaves the key usable everywhere else.
  </p>
{/if}

<style>
  /* A consequence, not a mistake — so `--text-faint` and no red. The palette keeps red for
     something being wrong, and a bare key is a choice this component deliberately allows. */
  .bare-note {
    margin: var(--gap-xs) 0 0;
    font-size: 0.78rem;
    line-height: 1.45;
    color: var(--text-faint);
  }
  .bind {
    display: flex;
    align-items: center;
    gap: var(--gap-sm);
    flex-wrap: wrap;
    font-size: 0.82rem;
  }
  .what {
    color: var(--text-dim);
  }
  /* Nothing bound reads as prose, not as a chord — a monospace "not set" looks like a key
     you could press. */
  .current.unset {
    font-family: var(--font);
    color: var(--text-faint);
    font-style: italic;
  }
  .current {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    background: var(--surface-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: 0.15rem 0.5rem;
  }
  .current:hover {
    border-color: var(--border-strong);
  }
  .current.recording {
    border-color: var(--accent-edge);
    background: var(--accent-wash);
    color: var(--accent);
  }
  .cancel {
    font-size: 0.7rem;
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text-faint);
    padding: 0.1rem 0.45rem;
  }
  .cancel:hover {
    color: var(--text-dim);
    border-color: var(--border-strong);
  }
  .error {
    font-size: 0.72rem;
    color: var(--red);
  }
</style>
