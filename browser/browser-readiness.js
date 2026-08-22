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

async function cdpHttpIsReady() {
  const versionResponse = await fetch(`${cdpUrl}/json/version`, {
    signal: AbortSignal.timeout(2000),
  });
  if (!versionResponse.ok) {
    throw new Error(`CDP version endpoint returned HTTP ${versionResponse.status}`);
  }

  const version = await versionResponse.json();
  if (typeof version.webSocketDebuggerUrl !== "string") {
    throw new Error("CDP version response has no WebSocket URL");
  }

  const listResponse = await fetch(`${cdpUrl}/json/list`, {
    signal: AbortSignal.timeout(2000),
  });
  if (!listResponse.ok) {
    throw new Error(`CDP target endpoint returned HTTP ${listResponse.status}`);
  }

  const targets = await listResponse.json();
  if (!Array.isArray(targets) || !targets.some((target) => target.type === "page")) {
    throw new Error("CDP has no page target");
  }

  return targets.length;
}

(async () => {
  await liveViewIsReady();

  // This is a liveness check, not an automation readiness check. Playwright's
  // connectOverCDP initializes every open tab; one frozen renderer can make a
  // healthy CDP service look dead and can also terminate active sessions.
  const targetCount = await cdpHttpIsReady();
  console.log(`Browser liveness OK (CDP targets: ${targetCount})`);
})().catch((error) => {
  console.error(`Browser readiness failed: ${error.message}`);
  process.exit(1);
});
