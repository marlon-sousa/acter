// Role: container (composition root) — wires the WebdriverIO runner to the built
// Acter binary. The only place E2E infrastructure is configured; specs see only the
// running app through the `browser` global.
//
// Session model: WebdriverIO talks DIRECTLY to the WebDriver server embedded in the
// app (tauri-plugin-wdio-webdriver, registered in debug builds only — see
// crates/acter-app/src/container.rs). No @wdio/tauri-service, no tauri-driver, no
// msedgedriver: the in-app server is a complete W3C endpoint, and the service layer
// was evaluated and dropped (see the T2 spec amendment — its session management
// added silent 5s probes for an optional companion plugin and pinned every worker
// to one shared app instance).
//
// Isolation model: one worker per spec file, and each worker spawns its OWN app
// instance on a unique port (beforeSession) and kills it afterwards (afterSession).
// Specs are fully independent; raising maxInstances parallelizes them safely.

import { spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

// The workspace target dir sits one level up from e2e/. `npm run test:e2e` builds
// the app before the runner starts (see the root script).
//
// The build MUST enable the `custom-protocol` feature and MUST be the debug
// profile. Tauri keys dev-vs-embedded assets on that feature, not on the profile:
// without it the app loads `devUrl` (the Vite dev server, not running under test)
// instead of the embedded frontend. Debug, because the embedded WebDriver plugin is
// registered under debug_assertions only — release binaries carry no automation
// surface. The frontend bundle is identical in both profiles.
const appBinaryPath = fileURLToPath(
  new URL('../target/debug/acter-app.exe', import.meta.url),
);

const BASE_PORT = 4600;

// The simulated session E2E runs against: the *built-in transcript with time taken
// out* (spec B6, decision 12).
//
// This used to be a hand-written `ACTER_FAKE_SCRIPT` config, and B6 deleted both the
// variable and the domain-level fake it configured. Faking is a transport choice now,
// so the deterministic-and-fast requirement T2 decision 8 stated has to be met at the
// transport: same shell, same rules, same text, same marker structure — only the waits
// are different. Every delay range becomes an equal-bounds 20 ms, which is what makes a
// run reproducible (unequal bounds are sampled per delivery, so `tail` and `burst`
// would otherwise pace themselves anywhere between three and eight seconds a chunk).
//
// Repeat counts are left exactly as they are, `forever` included: how many times a
// thing happens is part of the scenario, and `stop.spec.ts` needs a script that
// genuinely never ends.
//
// Reading the crate's transcript rather than copying it is deliberate. A copy would
// drift from the shell the manual accessibility matrix runs against, and then E2E would
// be asserting about a session nobody uses.
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

/** The built-in transcript with every wait replaced by a deterministic 20 ms. */
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

// Module state is per worker process (each spec file runs in its own worker, and
// the worker loads this config module independently).
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

  // Connection details are set per worker in beforeSession; these are placeholders
  // so the runner has a complete config before the hook runs.
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

  // Spawn this worker's private app instance on a unique port derived from the
  // worker id (cid "0-2" → worker index 2), then point the session at it.
  beforeSession: async (cfg, _capabilities, _specs, cid) => {
    const workerIndex = Number(cid?.split('-')[1] ?? 0);
    const port = BASE_PORT + workerIndex;

    // Write this worker's own copy of the fast transcript and point the app at it, so
    // E2E runs entirely on a session it generated (spec acceptance criterion 7). A file
    // per worker, because `ACTER_TRANSCRIPT` takes a path and workers are independent.
    const configDir = mkdtempSync(join(tmpdir(), 'acter-e2e-'));
    const configPath = join(configDir, 'transcript.json');
    writeFileSync(configPath, JSON.stringify(fastTranscript()));

    // `ACTER_SHELL` is cleared, not merely left unset. The parent environment is spread
    // in, and that variable exists precisely so a manual accessibility run can export it —
    // so a developer who did would have every spec here silently retargeted at a real
    // `cmd.exe`, where `forever` is not a command and the assertions mean nothing. A
    // suite that quietly tests a different session than it claims is worse than one that
    // fails.
    app = spawn(appBinaryPath, [], {
      env: {
        ...process.env,
        ACTER_SHELL: undefined,
        TAURI_WEBDRIVER_PORT: String(port),
        ACTER_TRANSCRIPT: configPath,
      },
      stdio: 'ignore',
    });
    await waitReady(port, 30_000);

    cfg.hostname = '127.0.0.1';
    cfg.port = port;
    cfg.path = '/';
  },

  afterSession: async () => {
    app?.kill();
    app = undefined;
  },

  // On any failure, drop a screenshot next to the run so CI can upload it as an
  // artifact (readable-output acceptance criterion).
  afterTest: async function (test, _context, { passed }) {
    if (!passed) {
      // Created on demand: the directory is not in the tree, and saveScreenshot fails
      // rather than creating it — which lost the artifact exactly when it was wanted.
      mkdirSync('./screenshots', { recursive: true });
      const safe = test.title.replace(/[^a-z0-9]+/gi, '-').toLowerCase();
      await browser.saveScreenshot(`./screenshots/${safe}.png`);
    }
  },
};
