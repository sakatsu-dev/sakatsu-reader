// @ts-check
/**
 * Reveal only the heading inside its own list. Scrolling an entire expanded entry can bring
 * its bottom into view and hide the heading; scrolling ancestors can move the app frame.
 * @param {HTMLElement} scroller
 * @param {HTMLElement} heading
 */
export function revealEntryHeading(scroller, heading) {
  const view = scroller.getBoundingClientRect();
  const head = heading.getBoundingClientRect();
  if (head.top < view.top) scroller.scrollTop += head.top - view.top;
  else if (head.bottom > view.bottom) {
    scroller.scrollTop +=
      head.height > view.height ? head.top - view.top : head.bottom - view.bottom;
  }
}
