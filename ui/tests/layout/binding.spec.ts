import type { Page } from "@playwright/test";
import { expect, test } from "./fixtures";
import { openSettings } from "./menu";
import { assertContained } from "./contained";

/**
 * Which mouse buttons may fire a capture.
 *
 * **Left and right click may not, and the reason is the reader rather than the mouse.** The
 * global hook deliberately never swallows an event — `binding.rs` calls `CallNextHookEx`
 * unconditionally, so a bound button goes on doing whatever it normally did. For every other
 * button that is what makes binding safe. For these two it is what makes it unusable: a left
 * click selects a word and a right click marks one Known, so binding either fires a capture on
 * top of a reading gesture made for a different reason, with no click left over to separate
 * them.
 *
 * Refused in three places, and this covers the one a person meets: `MouseButton::bindable` is
 * the authority, `rebind` refuses the command, and the recorder answers without a round trip.
 */

const recorder = (page: Page) => page.locator(".bind").filter({ hasText: "Capture" }).first();

/// The recorders live in Capture, so getting there is part of reaching them at all.
async function openCapture(page: Page) {
  await openSettings(page);
  await page.getByRole("button", { name: "Capture", exact: true }).first().click();
  await expect(recorder(page)).toBeVisible();
}

/** Start listening, then press `button` over the recorder. */
async function record(page: Page, button: "left" | "middle" | "right") {
  const bind = recorder(page);
  await bind.locator("button.current").click();
  await expect(bind.locator("button.current")).toHaveText("press anything…");
  // `mouse.click` rather than `locator.click`, because the recorder listens on the window and
  // a right click has to arrive as a real button press for `oncontextmenu` to see it.
  //
  // Over the recorder's own label, not the middle of its box. The recorder ignores a press on
  // its Cancel button — that press is how you stop — and where Cancel falls depends on how the
  // row wraps: in the narrower settings cards the box's centre landed on it at 620px and 480px.
  const box = await bind.locator(".what").boundingBox();
  await page.mouse.click(box!.x + box!.width / 2, box!.y + box!.height / 2, { button });
}

test("a reading click is refused, and the recorder keeps listening", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await openCapture(page);

  for (const button of ["right", "left"] as const) {
    await record(page, button);
    await expect(recorder(page).locator(".error")).toContainText(
      "is how the reader selects and marks words",
    );
    // **Still recording.** The refused press was deliberate, so dropping out would make
    // someone press Change again to reach the button they actually wanted.
    await expect(recorder(page).locator("button.current")).toHaveText("press anything…");
    await recorder(page).locator("button.cancel").click();
  }

  await assertContained(page);
});

/**
 * The half that would fail quietly.
 *
 * A rule that refused *every* mouse button would look the same in the settings page as one
 * that worked — the row would go on showing its previous binding either way. So the button
 * that is allowed has to be pressed, not assumed.
 */
test("the middle button is still bindable", async ({ page }) => {
  await page.goto("/?qa=analysis");
  await openCapture(page);

  await record(page, "middle");
  await expect(recorder(page).locator(".error")).toHaveCount(0);
  await expect(recorder(page).locator("button.current")).toHaveText("Middle click");
});
