// Role: port (driven) — what the controller needs from the live-region announcer.

export interface AnnouncerView {
  announce(text: string): void;
}
