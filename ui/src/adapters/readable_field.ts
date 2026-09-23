// Role: adapter (DOM) — a value a listener has to read character by character.
// Inside `role="application"` the arrows do not read prose, so the value must be a text field.
// Measured with NVDA 2026.1.1: a `readonly` input answers "blank" to every caret key, so every
// edit is refused instead.

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
  said.addEventListener('beforeinput', (event) => event.preventDefault());
  // For any path that reaches the value without a cancellable event.
  said.addEventListener('input', () => {
    said.value = value;
  });
  said.autocomplete = 'off';
  said.spellcheck = false;
  group.append(name, said);
  return group;
}
