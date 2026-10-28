// Copies a link with one tap. A button with `data-copy` copies the value of the field with that
// id, and says what happened in the element its `aria-describedby` names.
//
// The admin page swaps its links in with htmx, so clicks are handled for the whole document.

document.addEventListener("click", async (event) => {
  const button = event.target.closest("[data-copy]");
  if (!button) return;
  const field = document.getElementById(button.dataset.copy);
  const status = document.getElementById(button.getAttribute("aria-describedby"));
  try {
    await navigator.clipboard.writeText(field.value);
    status.textContent = "Copied. Paste it into the chat.";
  } catch (error) {
    console.error(error);
    // Selected, the link is one long-press away from the system's own copy.
    field.select();
    status.textContent = "Couldn't copy. The link is selected, so copy it from there.";
  }
});
