<script lang="ts">
  // What the app is and how to work it, for somebody who has just met it.
  //
  // Orientation rather than a manual. Every settings row carries one line saying what it does,
  // and the reasoning behind it under a More beside it; repeating either here would give two
  // places to disagree with each other — `settings-notes.test.mjs` fails if a More's sentence
  // turns up on this page. So this covers the shape of the thing instead: what the pages are
  // for, how text gets in, what the marks under the words mean, and which settings section
  // holds what, with a way straight into each. Anything that is one row in the settings belongs
  // in the settings.
  //
  // Read top to bottom once, so it scrolls rather than folds. A fold would put the answer
  // behind a click on the one page whose whole job is answering.

  import {
    SETTINGS_ABOUT,
    SETTINGS_SECTIONS,
    bindingLabel,
    type Binding,
    type SettingsSectionId,
  } from "../model";

  let {
    captureBind,
    overlayBind,
    switchBind,
    tabNavigation,
    backTo,
    onexit,
    onopensettings,
  }: {
    captureBind: Binding;
    overlayBind: Binding;
    /** Null until somebody binds it, which is the default. */
    switchBind: Binding | null;
    /** Which arrows walk the passage depends on it. */
    /** Whether conventional Windows Tab traversal is active. */
    tabNavigation: boolean;
    /** The page you came from, named — it goes in the way out's label. */
    backTo: string;
    onexit: () => void;
    /** Open Settings on one section. The way back from there is still the page before Help. */
    onopensettings: (section: SettingsSectionId) => void;
  } = $props();

  /**
   * Escape leaves, the same way the corner button does — the scheme Settings uses, because Help is
   * the same kind of page: somewhere you detour to and come back from.
   *
   * Simpler than Settings' copy by exactly one term. That page has three chord recorders waiting
   * for whatever you press, so its handler has to stand down while one is armed; this page has no
   * bindable input at all, so there is nothing to stand down for.
   *
   * `defaultPrevented` stays, and earns its place: the title bar's menu can be open over this page
   * and sees the same keystroke first. Whatever is on top closes before the page underneath does.
   */
  function onWindowKey(event: KeyboardEvent) {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    onexit();
  }
</script>

<svelte:window onkeydown={onWindowKey} />

<section class="page help">
  <h2>Help</h2>

  <!-- `tip-right` because the bubble would otherwise centre itself on a control at the page's
       right edge and hang off it. The button's rule and its reasoning are in `theme.css`. -->
  <button
    type="button"
    class="exit tip-right"
    onclick={onexit}
    aria-label={`Leave help and go back to ${backTo}`}
    data-tip={`Back to ${backTo}`}
  >
    <span aria-hidden="true">×</span>
  </button>

  <div class="page-body">
    <p class="lede">
      Sakatsu Reader turns Japanese you cannot yet read into Japanese you can work
      through — a word at a time, with the dictionary already open. It runs on your
      machine, and nothing you read leaves it unless you ask it to.
    </p>

    <section>
      <h3>Where everything is</h3>
      <p>
        Three pages down the left, and whichever one you are not in is the way back out of the
        one you are. The settings and this page come from the gear in the title bar.
      </p>
      <dl class="pages">
        <dt>Read</dt>
        <dd>
          A passage and its analysis. Put Japanese in the box, and it comes back broken
          into words you can click.
        </dd>
        <dt>Reference</dt>
        <dd>
          Looking something up on its own — a word, a kanji, a grammar point, a
          particle — with no passage involved. The words and the characters they are
          written with sit side by side, and the bar between them is yours to drag.
        </dd>
        <dt>Glossary</dt>
        <dd>
          Every grammar term the app can explain, to browse rather than to search. The
          filter narrows it as you type.
        </dd>
        <dt>About</dt>
        <dd>
          At the foot of the rail. What the app is built on, and the dictionaries and models
          behind what it tells you.
        </dd>
        <dt>Settings</dt>
        <dd>
          The gear at the top right, which lists the settings sections and opens straight into the
          one you pick. Each is a page with the rest listed beside it, and the box at the top
          searches all seven at once — type what you are after rather than guessing which
          section keeps it.
        </dd>
        <dt>Help</dt>
        <dd>This page, from that same gear.</dd>
      </dl>
    </section>

    <!-- **What each section is for, and a way into it — never what a row does.** Each row in the
         settings says that itself, with its reasoning under a More, and a second copy here would
         be the one that goes stale. The lines are `SETTINGS_ABOUT`, the same ones the settings
         page prints under each section's title, so the two cannot describe a section differently. -->
    <section>
      <h3>Settings, section by section</h3>
      <p>Open goes straight to the section. Each setting there explains itself.</p>
      <dl class="map">
        {#each SETTINGS_SECTIONS as entry}
          <dt>{entry.label}</dt>
          <dd>
            <span>{SETTINGS_ABOUT[entry.id]}</span>
            <button
              type="button"
              class="control"
              aria-label={`Open ${entry.label} settings`}
              onclick={() => onopensettings(entry.id)}>Open</button
            >
          </dd>
        {/each}
      </dl>
    </section>

    <section>
      <h3>Getting Japanese in</h3>
      <p>There are four ways, and they all end up in the same box.</p>
      <ul>
        <li>
          <strong>Type or paste it</strong> and press <kbd>Enter</kbd>.
        </li>
        <li>
          <strong>Capture part of the screen.</strong> Press
          <kbd>{bindingLabel(captureBind)}</kbd> and drag a box around some text — a game,
          a manga page, a subtitle. The app reads the picture and analyses what it finds.
        </li>
        <li>
          <strong>Take whatever is on the clipboard</strong>, with the button in the top
          right. Useful when you can already select the text.
        </li>
        <li>
          <strong>Leave the overlay sitting over the text.</strong> It is a small frame
          you park where the Japanese appears, so capturing the same spot again is one
          keypress instead of a drag.
        </li>
      </ul>
      <p class="aside">
        The overlay button is in the top right, next to the clipboard one. Clicking it
        shows or hides the frame; double-clicking switches the whole feature off, and
        double-clicking again brings it back — the button turns red while it is off. The
        lock on the frame itself stops it jumping to your mouse when you show it.
      </p>
    </section>

    <section>
      <h3>Your shortcuts</h3>
      <p>
        The first three are yours to change, under <em>Capture</em> in the settings.
        These are what they are set to now.
      </p>
      <dl class="keys">
        <dt>Capture a region of the screen</dt>
        <dd><kbd>{bindingLabel(captureBind)}</kbd></dd>

        <dt>Show or hide the overlay</dt>
        <dd><kbd>{bindingLabel(overlayBind)}</kbd></dd>

        <!-- The same key, and worth its own row for that reason: three gestures on one
             binding is the kind of thing nobody finds by pressing it. -->
        <dt>Hold it instead, to put the overlay away</dt>
        <dd><kbd>{bindingLabel(overlayBind)}</kbd> <span class="unset">held</span></dd>

        <dt>Tap it twice, to lock the overlay where it lands</dt>
        <dd><kbd>{bindingLabel(overlayBind)}</kbd> <span class="unset">twice</span></dd>

        <dt>Switch the overlay off, or back on</dt>
        <dd>
          {#if switchBind}
            <kbd>{bindingLabel(switchBind)}</kbd>
          {:else}
            <!-- Prose, not monospace. A "not set" in a key cap looks like a key you could
                 press, which is the opposite of what it says. -->
            <span class="unset">not set</span>
          {/if}
        </dd>

        <dt>Analyse what is in the box</dt>
        <dd><kbd>Enter</kbd></dd>

        <dt>Start a new line instead</dt>
        <dd><kbd>Shift</kbd> <kbd>Enter</kbd></dd>

        <dt>Step through the words</dt>
        <dd><kbd>→</kbd> <kbd>←</kbd></dd>

        <dt>Move through controls</dt>
        <dd>
          {#if tabNavigation}
            <kbd>Tab</kbd> <span class="unset">on</span>
          {:else}
            <span class="unset">off — enable Windows keyboard navigation in General settings</span>
          {/if}
        </dd>
      </dl>

    </section>

    <!-- Its own section, matching every other group here: one `h3` and one `dl`. The page styles
         `h3` and nothing below it, so an `h4` would have been an unstyled heading. -->
    <section>
      <h3>Searches on the Reference page</h3>
      <p>
        Several searches stay open at once, and these are the chords a browser already uses for
        the same thing. They are fixed rather than yours to change.
      </p>
      <dl class="keys">
        <dt>New search tab</dt>
        <dd><kbd>Ctrl</kbd> <kbd>T</kbd></dd>

        <dt>Close this tab</dt>
        <dd><kbd>Ctrl</kbd> <kbd>W</kbd></dd>

        <dt>Reopen the last one closed</dt>
        <dd><kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>T</kbd></dd>

        <dt>Next tab, previous tab</dt>
        <dd><kbd>Ctrl</kbd> <kbd>Tab</kbd> · <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>Tab</kbd></dd>

        <dt>Jump to a tab</dt>
        <dd><kbd>Ctrl</kbd> <kbd>1</kbd> … <kbd>Ctrl</kbd> <kbd>8</kbd></dd>

        <dt>Jump to the last tab</dt>
        <dd><kbd>Ctrl</kbd> <kbd>9</kbd></dd>
      </dl>
    </section>

    <section>
      <h3>What the marks mean</h3>
      <p>
        Every word starts new, and you say what it becomes. Only the unfinished states
        are marked, so a passage you have worked through gets quieter rather than
        busier.
      </p>
      <dl class="marks">
        <dt><span class="token status-new">言葉</span></dt>
        <dd>New — worth a look.</dd>

        <dt><span class="token status-seen">読む</span></dt>
        <dd>Seen — met before, not settled yet.</dd>

        <dt><span class="token">本</span></dt>
        <dd>Known. No mark at all; that is the point of it.</dd>

        <dt><span class="token status-ignored">私</span></dt>
        <dd>Ignored — faded out of the way, because you asked it to be quiet.</dd>

        <dt><span class="token pattern">なければ</span></dt>
        <dd>
          Part of a grammar point the app can explain. Its own dotted line, since a word
          is often both this and new.
        </dd>

        <dt>
          <span class="token ambiguous"><ruby>今日<rt>きょう</rt></ruby></span>
        </dt>
        <dd>
          The reading is a guess. Either the dictionary gives this spelling more than one and the
          app chose, or nothing confirmed it. Worth checking before you learn it.
        </dd>
      </dl>
      <p>
        Click a New word or reach it with the arrow keys to mark it Seen. Right-click a
        word to mark it Known without moving the open definition. You can turn automatic
        Seen marking off under General in Settings; every status is remembered afterwards.
      </p>
    </section>

    <section>
      <h3>Installation and extras</h3>
      <p>
        The app can analyse pasted Japanese with nothing installed because the analyzer is
        built in. A complete setup adds dictionaries plus both required recognition engines:
        <strong>PP-OCRv6</strong> for ordinary game and menu text, and
        <strong>manga-ocr</strong> for manga, vertical text and hand lettering.
        Optional extras include <strong>translation</strong> for a whole line at once, and a
        <strong>frequency list</strong> so common words sort above obscure ones.
      </p>
      <p>
        They live in the settings, under the gear in the title bar. If you dismissed the setup
        panel on the reading page, everything it offered is still there: the required downloads
        under <em>Recognition</em>, and the extras under <em>General</em>.
      </p>
    </section>

    <p class="last">
      Everything else — text size, furigana, how the page is laid out, Anki — is under the gear
      in the title bar.
    </p>
  </div>
</section>

<style>
  /* The `.exit` button is positioned against this. */
  .help {
    position: relative;
  }

  /* Sections flow into as many columns as the width allows.
   *
   * They used to stack in one column capped at a readable measure, which is right for the
   * line length and wrong for the page: at a wide reading column it left the help in the
   * left quarter with nothing beside it. The column *is* the measure now — `minmax` sets
   * the narrowest a column may be and `auto-fill` decides how many fit, so a wide window
   * gets more columns rather than longer lines, and a narrow one collapses to a single
   * column with nothing to configure.
   *
   * The same shape the glossary's card view uses, for the same reason. */
  .page-body {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(26rem, 1fr));
    /* Wider between columns than between rows. At the same gap, two columns of prose sit
       close enough to read as one wrapping paragraph. */
    gap: var(--gap-lg) calc(var(--gap-lg) * 2);
    /* Sections are their own height rather than their row's, and the rows sit at the top
       rather than spreading down a tall page. */
    align-items: start;
    align-content: start;
  }

  /* The opening and the closing line frame the rest, so they run the whole width instead
     of becoming the first and last cells of the grid. */
  .lede,
  .last {
    grid-column: 1 / -1;
  }

  p {
    margin: 0;
    color: var(--text-dim);
    line-height: 1.6;
    font-size: 0.9rem;
  }
  .lede {
    color: var(--text);
    font-size: 0.95rem;
    /* Spanning every column, this one still needs a measure of its own — it is the only
       paragraph on the page a column is not already sizing. */
    max-width: 42rem;
  }
  .aside {
    color: var(--text-faint);
    font-size: 0.85rem;
  }
  .last {
    color: var(--text-faint);
    font-size: 0.85rem;
    padding-top: var(--gap);
    border-top: 1px solid var(--border);
  }

  section {
    /* What the `dl` below asks about. Its width is now a grid column's, which has nothing
       to do with the window's — see `local documentation`. */
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    gap: var(--gap-sm);
  }
  h3 {
    margin: 0;
    font-size: 0.88rem;
    font-weight: 600;
    color: var(--text);
  }

  ul {
    margin: 0;
    padding-left: 1.1rem;
    display: flex;
    flex-direction: column;
    gap: var(--gap-sm);
    color: var(--text-dim);
    line-height: 1.6;
    font-size: 0.9rem;
  }
  strong {
    color: var(--text);
    font-weight: 600;
  }
  em {
    color: var(--text);
    font-style: normal;
  }

  /* Term on the left, what it is on the right. Collapses to stacked rows when the pane is
     too narrow to hold both, which it often is beside the reader. */
  dl {
    margin: 0;
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--gap-sm) var(--gap);
    align-items: baseline;
    font-size: 0.9rem;
    line-height: 1.6;
  }
  dt {
    color: var(--text);
    font-weight: 600;
  }
  dd {
    margin: 0;
    color: var(--text-dim);
  }
  .keys dt,
  .marks dt {
    font-weight: 400;
    color: var(--text-dim);
  }
  /* Action on the left, key on the right — the order BindRecorder puts the same two
     things in, so the settings panel and this page read the same way round. */
  .keys dd,
  .marks dt {
    justify-self: start;
  }

  /* `@container`, not `@media`. These sit in a grid column that can be 26rem inside a
     window three times that, so asking the window would keep two columns in a space with
     room for one. */
  @container (max-width: 26rem) {
    dl {
      grid-template-columns: 1fr;
      gap: var(--gap-xs);
    }
    dd + dt {
      margin-top: var(--gap-sm);
    }
  }

  /* A section's line, then its way in. The button keeps its own width and sits at the end of
     the line, so the seven Opens form a column you can run down rather than a ragged edge. */
  .map dd {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--gap);
  }
  .map button {
    flex: 0 0 auto;
    font-size: 0.75rem;
    padding: 0.2rem 0.6rem;
  }

  /* Lifted from `.current` in BindRecorder, so a chord looks the same wherever it is
     shown. Two drawings of one thing would be two things to keep in step. */
  kbd {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    background: var(--surface-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: 0.15rem 0.5rem;
    white-space: nowrap;
  }
  .unset {
    color: var(--text-faint);
    font-style: italic;
    font-size: 0.85rem;
  }

  /* The real thing, not a description of it. These carry the same rules `.token` does in
     Reader.svelte — if those change and these do not, the page is lying, which is worse
     than saying nothing. **It has lied twice**: the rules there gained a square bottom and then a
     gap between the two marks, and this copy sat unchanged through both.

     What has to match is the *appearance*, not the declarations. These are 15px spans in prose and
     those are 28px words in a 2.5 line box, so the reader needs a `line-height` override and an
     `em` offset to undo its own leading, and this needs neither — an inline element's border box
     is font-derived, so it already hugs its glyphs. The numbers below are in px for that reason:
     at this size `0.2em` would put the dashed strip through the bottom of the characters. */
  .token {
    font-family: var(--font-jp);
    border-bottom: 2px solid transparent;
    /* Square along the bottom, so the rule is an underline and not the foot of a tile. */
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
    /* Deep enough to hold the gap between the dashed strip and the rule below it. */
    padding: 0 2px 5px;
    color: var(--text);
  }
  .token.status-new {
    border-bottom-color: var(--blue-edge);
  }
  .token.status-seen {
    border-bottom-color: var(--yellow);
  }
  .token.status-ignored {
    color: var(--text-faint);
  }
  /* **Ruby, only here, and only because the legend has to show the thing it names.**
     `Reader.svelte`'s `rt` rule is scoped to it, so a `<rt>` in this file arrives with the
     browser's defaults — half the size and the wrong colour. These four declarations are the
     reader's, copied, and `reading-caveat.spec.ts` compares the two computed styles rather than
     trusting that they stayed in step. The comment at the top of this block says this file has
     lied twice; that test is the answer to it. */
  rt {
    font-size: 0.4em;
    color: var(--text-faint);
    font-family: var(--font-jp);
    user-select: none;
  }
  .token.ambiguous rt {
    text-decoration: underline dotted var(--text-dim);
    text-underline-offset: 0.16em;
  }
  /* Lifted clear of the status rule, as in the reader: flush on top of it the two read as one
     thick line with a speckled edge rather than as a dotted line meaning something of its own. */
  .token.pattern {
    background-image: linear-gradient(90deg, var(--text-dim) 50%, transparent 50%);
    background-size: 5px 1px;
    background-repeat: repeat-x;
    background-position: 0 calc(100% - 3px);
    background-clip: padding-box;
  }
</style>
