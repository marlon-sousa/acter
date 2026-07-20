// Role: adapter (WebAudio) — the decided home of the completion beep (decision 1): a
// short 880 Hz tone. The AudioContext is created lazily on first beep (browsers block
// audio until a user gesture, and the first beep only follows a submitted command) and
// reused thereafter. A brief gain envelope avoids the click of a hard start/stop.

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
    // Fade in and out over a few milliseconds so the tone starts and stops cleanly.
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
