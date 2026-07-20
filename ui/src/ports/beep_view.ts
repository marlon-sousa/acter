// Role: port (driven) — the completion beep. The beep renders the too-big verdict
// audibly, exactly as the live region renders it in speech (decision 1); all audible
// output lives behind this one seam so a manual finding maps to one small adapter.

export interface BeepView {
  beep(): void;
}
