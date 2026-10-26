/** App-level navigation request used by term notes nested deep inside vocabulary/grammar. */
export const GLOSSARY_OPEN = "sakatsu-open-glossary";

export function openGlossary(id: string) {
  window.dispatchEvent(new CustomEvent<string>(GLOSSARY_OPEN, { detail: id }));
}
