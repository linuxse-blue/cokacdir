const { chromium } = require("/usr/local/lib/node_modules/playwright-core");
const { execFileSync } = require("node:child_process");

const cdpUrl = "http://127.0.0.1:9222";
const noVncUrl = "http://127.0.0.1:6080/vnc.html";

async function liveViewIsReady() {
  execFileSync("supervisorctl", ["status", "x11vnc", "novnc"], {
    stdio: "ignore",
    timeout: 2000,
  });
  const response = await fetch(noVncUrl, {
    signal: AbortSignal.timeout(2000),
  });
  if (!response.ok) {
    throw new Error(`noVNC returned HTTP ${response.status}`);
  }
}

async function probe() {
  const browser = await chromium.connectOverCDP(cdpUrl, { timeout: 5000 });
  try {
    const contexts = browser.contexts();
    if (contexts.length === 0) {
      throw new Error("CDP connection has no browser context");
    }

    const pages = contexts.flatMap((context) => context.pages());
    if (pages.length === 0) {
      throw new Error("CDP connection has no page");
    }

    await pages[0].evaluate(() => document.readyState);
  } finally {
    await browser.close();
  }
}

async function proxyHttpIsReady() {
  try {
    const response = await fetch(`${cdpUrl}/json/version`, {
      signal: AbortSignal.timeout(2000),
    });
    return response.ok;
  } catch {
    return false;
  }
}

(async () => {
  await liveViewIsReady();

  try {
    await probe();
  } catch (error) {
    if (!(await proxyHttpIsReady())) throw error;

    console.error(`CDP probe failed; restarting proxy: ${error.message}`);
    execFileSync("supervisorctl", ["restart", "kernel-images-api"], {
      stdio: "inherit",
      timeout: 5000,
    });
    await new Promise((resolve) => setTimeout(resolve, 500));
    await probe();
  }
})().catch((error) => {
  console.error(`Browser readiness failed: ${error.message}`);
  process.exit(1);
});
