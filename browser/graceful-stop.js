const endpoint = "http://127.0.0.1:9223/json/version";

(async () => {
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(`CDP endpoint returned HTTP ${response.status}`);
  }

  const { webSocketDebuggerUrl } = await response.json();
  if (!webSocketDebuggerUrl) {
    throw new Error("CDP endpoint has no WebSocket URL");
  }

  await new Promise((resolve, reject) => {
    const socket = new WebSocket(webSocketDebuggerUrl);
    const timeout = setTimeout(() => {
      socket.close();
      reject(new Error("Browser.close timed out"));
    }, 8000);

    socket.addEventListener("open", () => {
      socket.send(JSON.stringify({ id: 1, method: "Browser.close" }));
    });
    socket.addEventListener("message", (event) => {
      const message = JSON.parse(event.data);
      if (message.id === 1) {
        clearTimeout(timeout);
        resolve();
      }
    });
    socket.addEventListener("close", () => {
      clearTimeout(timeout);
      resolve();
    });
    socket.addEventListener("error", () => {
      clearTimeout(timeout);
      reject(new Error("Browser.close WebSocket failed"));
    });
  });
})().catch((error) => {
  console.error(`Graceful browser shutdown failed: ${error.message}`);
  process.exit(1);
});
