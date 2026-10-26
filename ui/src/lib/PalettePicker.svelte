<script lang="ts">
  // The list of palettes, drawn once for the two places that offer it.
  //
  // **It was written twice, and had already drifted.** The settings page and the title bar's menu
  // each carried their own copy of this `<select>`: renaming the groups meant editing both, and the
  // menu's copy applied a palette's *colours* while the page's also applied its decoration — so the
  // same palette gave two different results depending on which surface you picked it from.
  //
  // The markup is here; what happens when you pick is `App.svelte`'s, which is the component that
  // owns both the theme and the thirteen decoration values a palette can carry. One list, one
  // answer, and nothing left to keep in step.
  import { PRESETS, matchPalette, palettesFor } from "./theme.js";
  import type { NamedTheme, ThemeSettings } from "../model";

  let {
    theme,
    themePresets,
    onpick,
  }: {
    theme: ThemeSettings;
    themePresets: NamedTheme[];
    /// Named rather than applied here: see above.
    onpick: (name: string) => void;
  } = $props();

  const CUSTOM = "custom";
  const palettes = $derived(palettesFor(PRESETS, themePresets));
  /// Which palette this theme *is* — compared rather than remembered, so nothing can go stale.
  const current = $derived(matchPalette(palettes, theme) ?? CUSTOM);
</script>

<select value={current} onchange={(event) => onpick(event.currentTarget.value)}>
  <optgroup label="Built-in">
    {#each PRESETS as preset}<option value={preset.name}>{preset.name}</option>{/each}
  </optgroup>
  {#if themePresets.length > 0}
    <optgroup label="Custom">
      {#each themePresets as saved}<option value={saved.name}>{saved.name}</option>{/each}
    </optgroup>
  {/if}
  <!-- Shown only once the dials sit on no saved palette. Not selectable: there is nothing to
       switch *to*, it is where you already are.

       **"Unsaved", not "Custom".** The saved palettes are grouped under Custom, and a row wearing
       the same word as the group above it says nothing twice — the settings page had that exact
       fault as `Palette › Palette` and lost it. This means the dials match no saved palette at
       all, which "Unsaved" says and "Custom" only implied. -->
  {#if current === CUSTOM}<option value={CUSTOM} disabled>Unsaved</option>{/if}
</select>
