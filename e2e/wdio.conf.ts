// Role: container (composition root) — wires the WebdriverIO runner to the built Acter binary.

import { spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

// The build must enable `custom-protocol` and be the debug profile: without the feature the
// app loads `devUrl`, and the embedded WebDriver server exists in debug builds only.
const appBinaryPath = fileURLToPath(
  new URL('../target/debug/acter.exe', import.meta.url),
);

const BASE_PORT = 4600;

const TRANSCRIPT_SOURCE = fileURLToPath(
  new URL(
    '../crates/acter-transports/src/scripted/default_transcript.json',
    import.meta.url,
  ),
);

// Equal bounds, so a delivery's wait is a constant rather than a sample.
const FAST_MS = { min_ms: 20, max_ms: 20 };

interface Step {
  delay?: { min_ms: number; max_ms: number };
  [key: string]: unknown;
}

interface Rule {
  steps?: Step[];
  [key: string]: unknown;
}

interface Transcript {
  on_start?: Step[];
  rules?: Rule[];
  default?: Rule;
  [key: string]: unknown;
}

function fastTranscript(): Transcript {
  const transcript = JSON.parse(
    readFileSync(TRANSCRIPT_SOURCE, 'utf8'),
  ) as Transcript;
  const hurry = (steps: Step[] | undefined): void => {
    for (const step of steps ?? []) {
      if (step.delay !== undefined) {
        step.delay = { ...FAST_MS };
      }
    }
  };
  hurry(transcript.on_start);
  for (const rule of transcript.rules ?? []) {
    hurry(rule.steps);
  }
  hurry(transcript.default?.steps);
  return transcript;
}

// Module state is per worker: each spec file runs in its own worker process.
let app: ChildProcess | undefined;

async function waitReady(port: number, timeoutMs: number): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/status`, {
        signal: AbortSignal.timeout(2000),
      });
      if (res.ok) {
        const data = (await res.json()) as { value?: { ready?: boolean } };
        if (data.value?.ready === true) {
          return;
        }
      }
    } catch {
      // Server not up yet; keep polling.
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(
    `Acter's embedded WebDriver server did not become ready on port ${port} ` +
      `within ${timeoutMs}ms. Was the app built with --features custom-protocol ` +
      `in the debug profile?`,
  );
}

export const config: WebdriverIO.Config = {
  runner: 'local',
  tsConfigPath: fileURLToPath(new URL('tsconfig.json', import.meta.url)),

  specs: ['./test/specs/**/*.spec.ts'],
  maxInstances: 1,

  // Placeholders: beforeSession sets the real port for each worker.
  hostname: '127.0.0.1',
  port: BASE_PORT,
  path: '/',

  capabilities: [
    {
      browserName: 'tauri',
    },
  ],

  logLevel: 'warn',
  bail: 0,
  waitforTimeout: 10_000,
  connectionRetryTimeout: 60_000,
  connectionRetryCount: 2,

  framework: 'mocha',
  mochaOpts: {
    ui: 'bdd',
    timeout: 30_000,
  },

  reporters: ['spec'],

  beforeSession: async (cfg, _capabilities, _specs, cid) => {
    const workerIndex = Number(cid?.split('-')[1] ?? 0);
    const port = BASE_PORT + workerIndex;

    const configDir = mkdtempSync(join(tmpdir(), 'acter-e2e-'));
    const configPath = join(configDir, 'transcript.json');
    writeFileSync(configPath, JSON.stringify(fastTranscript()));

    const settingsDir = join(configDir, 'settings');
    mkdirSync(settingsDir, { recursive: true });
    writeFileSync(
      join(settingsDir, 'settings.json'),
      JSON.stringify({
        format: 1,
        connections: [
          {
            name: 'the fake',
            target: { target: 'Scripted', scenario: 'builtin' },
            set_up: 'Yes',
            line_owner: 'FarEnd',
          },
        ],
        host_keys: [],
        offer_to_save: true,
      }),
    );

    // `ACTER_SHELL` is cleared because a manual accessibility run may export it, which would
    // retarget every spec at a real `cmd.exe`.
    app = spawn(appBinaryPath, [], {
      env: {
        ...process.env,
        ACTER_SHELL: undefined,
        TAURI_WEBDRIVER_PORT: String(port),
        ACTER_TRANSCRIPT: configPath,
        ACTER_SETTINGS_DIR: settingsDir,
      },
      stdio: 'ignore',
    });
    await waitReady(port, 30_000);

    cfg.hostname = '127.0.0.1';
    cfg.port = port;
    cfg.path = '/';
  },

  // A session starts with the program holding the keys, and the specs type into Acter's own
  // `<input>`, which stays hidden until Ctrl+Shift+K takes them back.
  before: async () => {
    const browser = (globalThis as { browser: WebdriverIO.Browser }).browser;
    await browser.waitUntil(
      async () =>
        await browser.execute(
          () => document.getElementById('far-end-line')?.hidden === false,
        ),
      {
        timeout: 30_000,
        timeoutMsg:
          'a session did not hand the keys to the program, which is the default since 28.7',
      },
    );
    await browser.execute(() => {
      document.dispatchEvent(
        new KeyboardEvent('keydown', {
          key: 'K',
          ctrlKey: true,
          shiftKey: true,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    await browser.waitUntil(
      async () =>
        await browser.execute(
          () => document.getElementById('far-end-line')?.hidden === true,
        ),
      {
        timeout: 15_000,
        timeoutMsg: 'Ctrl+Shift+K did not bring the keys back to Acter',
      },
    );
  },

  afterSession: async () => {
    app?.kill();
    app = undefined;
  },

  afterTest: async function (test, _context, { passed }) {
    if (!passed) {
      // saveScreenshot fails rather than create a missing directory.
      mkdirSync('./screenshots', { recursive: true });
      const safe = test.title.replace(/[^a-z0-9]+/gi, '-').toLowerCase();
      await browser.saveScreenshot(`./screenshots/${safe}.png`);
    }
  },
};
