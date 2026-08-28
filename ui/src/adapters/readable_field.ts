// Role: adapter (DOM) — a value a listener has to read character by character.
//
// **Reported by the user on 2026-08-26, and it was a real defect rather than a preference.**
// A fingerprint used to be a focusable `<code>`, on the reasoning that a value read character
// by character needs somewhere to put a cursor. That reasoning was half right and the
// conclusion was wrong: these dialogs are inside `role="application"`, where the arrows do
// **not** read prose, so the only way to walk a paragraph is NVDA's review cursor — which
// `screenreader://guidance` places outside the vocabulary an ordinary user is assumed to
// have. The thing it most needed to be comparable was the thing it was not.
//
// **And `readonly` is not the fix either.** Measured with NVDA 2026.1.1 on 2026-08-26: with
// `readonly` set, the field has focus and reports its value through the accessibility API —
// role EDITABLETEXT, states READONLY FOCUSABLE FOCUSED, the full value — and yet **every**
// caret key answered "blank": Right, Left, Home and End alike. The same dialog's editable
// Host field, arrowed in the same session, read "1", "2", "2". So a read-only input in this
// webview exposes a value with no caret-navigable text behind it, and a fingerprint nobody
// can walk is a fingerprint nobody can compare.
//
// What works is an editable field whose every edit is refused: the caret is a real caret and
// the value cannot change.
//
// It lives in its own module because two dialogs need it — the host key's fingerprints since
// B9, and the file path and signer of an unverified program since B5.7 — and because two
// copies of a measured accessibility fix are two things that can drift, which is
// `dialog_tab`'s reasoning applied to the other measurement of the same afternoon.

/**
 * A labelled value, as a text box that can be walked and cannot be changed.
 *
 * `id` is the element's, so a dialog can find it again to place focus on it.
 */
export function readableField(
  document: Document,
  id: string,
  label: string,
  value: string,
): HTMLElement {
  const group = document.createElement('p');
  const name = document.createElement('label');
  name.htmlFor = id;
  name.textContent = label;
  const said = document.createElement('input');
  said.id = id;
  said.type = 'text';
  said.value = value;
  // Refusing the edit at `beforeinput` keeps the caret a real caret while making the value
  // unchangeable — including by paste, which is one of the input types this cancels.
  said.addEventListener('beforeinput', (event) => event.preventDefault());
  // Belt and braces, for any path that reaches the value without a cancellable event.
  said.addEventListener('input', () => {
    said.value = value;
  });
  // Nothing here is a thing to fill in, and a browser offering to complete a fingerprint or
  // a file path would be offering the one value that must not come from anywhere but the
  // thing being described.
  said.autocomplete = 'off';
  said.spellcheck = false;
  group.append(name, said);
  return group;
}
