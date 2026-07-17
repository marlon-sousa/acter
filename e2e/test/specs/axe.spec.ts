// Role: e2e spec — axe-core injected into the real WebView2. Asserts zero
// critical/serious accessibility violations on the running app. DOM-semantics
// regressions (missing names, bad roles, live-region misuse) become machine-caught;
// NVDA speech stays a manual pass (out of scope, per DESIGN.md).

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
    // Populate the buffer first, so axe audits the app in a real used state
    // (heading + response present), not just the empty shell.
    await submitCommand('audit me');
    await $('#results h2').waitForExist({ timeout: 10_000 });

    // Inject the axe-core library into the page, then run it there. The embedded
    // WebDriver's execute endpoint awaits returned promises, so a plain async
    // execute works (executeAsync is not needed).
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
      // Readable failure output: rule, impact, and the offending selectors.
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
