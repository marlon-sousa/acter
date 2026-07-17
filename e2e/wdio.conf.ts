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

    app = spawn(appBinaryPath, [], {
      env: { ...process.env, TAURI_WEBDRIVER_PORT: String(port) },
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
      const safe = test.title.replace(/[^a-z0-9]+/gi, '-').toLowerCase();
      await browser.saveScreenshot(`./screenshots/${safe}.png`);
    }
  },
};
