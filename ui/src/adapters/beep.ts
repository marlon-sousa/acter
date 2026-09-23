// Role: adapter (WebAudio) — the completion beep.
// The AudioContext must be created lazily: browsers block audio until a user gesture.

import type { BeepView } from '../ports/beep_view';

const FREQUENCY_HZ = 880;
const DURATION_S = 0.15;

export class BeepAudio implements BeepView {
  private context: AudioContext | undefined;

  beep(): void {
    const context = (this.context ??= new AudioContext());
    const now = context.currentTime;

    const oscillator = context.createOscillator();
    oscillator.frequency.value = FREQUENCY_HZ;

    const gain = context.createGain();
    gain.gain.setValueAtTime(0, now);
    gain.gain.linearRampToValueAtTime(0.2, now + 0.01);
    gain.gain.setValueAtTime(0.2, now + DURATION_S - 0.01);
    gain.gain.linearRampToValueAtTime(0, now + DURATION_S);

    oscillator.connect(gain);
    gain.connect(context.destination);
    oscillator.start(now);
    oscillator.stop(now + DURATION_S);
  }
}
