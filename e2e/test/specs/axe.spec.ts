// Role: e2e spec — axe-core injected into the real WebView2, asserting zero critical or
// serious accessibility violations on the running app.

import axe from 'axe-core';
import { $, browser, expect } from '@wdio/globals';

import { submitCommand } from '../helpers';

interface AxeViolation {
  id: string;
  impact: string | null;
  help: string;
  nodes: { target: string[] }[];
}

interface AxeResults {
  violations: AxeViolation[];
}

describe('axe-core: no critical or serious violations', () => {
  it('passes an axe audit of the running app', async () => {
    await submitCommand('audit me');
    await $('#results h2').waitForExist({ timeout: 10_000 });

    // The embedded WebDriver's execute endpoint awaits a returned promise.
    await browser.execute(axe.source);
    const results = (await browser.execute(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return (window as any).axe.run(document, { resultTypes: ['violations'] });
    })) as AxeResults;

    const blocking = results.violations.filter(
      (violation) =>
        violation.impact === 'critical' || violation.impact === 'serious',
    );

    if (blocking.length > 0) {
      const report = blocking
        .map(
          (violation) =>
            `${violation.impact} — ${violation.id}: ${violation.help}\n  ${violation.nodes
              .map((node) => node.target.join(' '))
              .join('\n  ')}`,
        )
        .join('\n');
      console.error(`axe found blocking violations:\n${report}`);
    }

    expect(blocking).toEqual([]);
  });
});
