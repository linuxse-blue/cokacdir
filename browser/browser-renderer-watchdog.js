const { execFileSync } = require("node:child_process");

const cdpUrl = "http://127.0.0.1:9223";
const intervalMs = Number.parseInt(
  process.env.BROWSER_RENDERER_WATCHDOG_INTERVAL_MS || "30000",
  10,
);
const commandTimeoutMs = Number.parseInt(
  process.env.BROWSER_RENDERER_WATCHDOG_COMMAND_TIMEOUT_MS || "3000",
  10,
);
const failureThreshold = Number.parseInt(
  process.env.BROWSER_RENDERER_WATCHDOG_FAILURE_THRESHOLD || "3",
  10,
);

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function listPageTargets() {
  const response = await fetch(`${cdpUrl}/json/list`, {
    signal: AbortSignal.timeout(commandTimeoutMs),
  });
  if (!response.ok) {
    throw new Error(`CDP target endpoint returned HTTP ${response.status}`);
  }

  const targets = await response.json();
  if (!Array.isArray(targets)) {
    throw new Error("CDP target endpoint returned a non-array response");
  }

  return targets.filter(
    (target) => target.type === "page" && typeof target.webSocketDebuggerUrl === "string",
  );
}

function probePage(target) {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(target.webSocketDebuggerUrl);
    const commandId = 1;
    let settled = false;

    const finish = (callback, value) => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(timer);
      try {
        socket.close();
      } catch {
        // The socket may already be closed after a renderer failure.
      }
      callback(value);
    };

    const timer = setTimeout(() => {
      finish(reject, new Error("renderer command timed out"));
    }, commandTimeoutMs);

    socket.addEventListener("open", () => {
      socket.send(
        JSON.stringify({
          id: commandId,
          method: "Runtime.evaluate",
          params: {
            expression: "document.readyState",
            returnByValue: true,
          },
        }),
      );
    });
    socket.addEventListener("message", (event) => {
      let message;
      try {
        message = JSON.parse(event.data);
      } catch {
        return;
      }

      if (message.id !== commandId) {
        return;
      }
      if (message.error) {
        finish(reject, new Error(message.error.message || "CDP command failed"));
        return;
      }
      finish(resolve);
    });
    socket.addEventListener("error", () => {
      finish(reject, new Error("renderer WebSocket failed"));
    });
    socket.addEventListener("close", () => {
      if (!settled) {
        finish(reject, new Error("renderer WebSocket closed before response"));
      }
    });
  });
}

function restartChromium() {
  console.error("[watchdog] restarting Chromium after repeated renderer failures");
  execFileSync("supervisorctl", ["restart", "chromium"], {
    stdio: "inherit",
    timeout: 15000,
  });
}

async function run() {
  let consecutiveFailures = 0;

  console.log(
    `[watchdog] enabled (interval=${intervalMs}ms, commandTimeout=${commandTimeoutMs}ms, threshold=${failureThreshold})`,
  );

  while (true) {
    try {
      const targets = await listPageTargets();
      const failures = [];

      for (const target of targets) {
        try {
          await probePage(target);
        } catch (error) {
          failures.push(`${target.title || target.url}: ${error.message}`);
        }
      }

      if (failures.length === 0) {
        consecutiveFailures = 0;
      } else {
        consecutiveFailures += 1;
        console.error(
          `[watchdog] renderer probe failed (${consecutiveFailures}/${failureThreshold}) on ${failures.length} page(s)`,
        );
        if (consecutiveFailures >= failureThreshold) {
          restartChromium();
          consecutiveFailures = 0;
          await sleep(10000);
        }
      }
    } catch (error) {
      console.error(`[watchdog] probe unavailable: ${error.message}`);
    }

    await sleep(intervalMs);
  }
}

run().catch((error) => {
  console.error(`[watchdog] stopped: ${error.message}`);
  process.exit(1);
});
