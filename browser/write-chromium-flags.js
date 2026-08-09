const fs = require("fs");

const chromiumFlags = [
  "--user-data-dir=/home/kernel/user-data",
  "--disable-dev-shm-usage",
  "--start-maximized",
  "--remote-allow-origins=*",
];

if (process.env.BROWSER_ENABLE_WEBGL === "1") {
  chromiumFlags.push(
    "--enable-webgl",
    "--use-angle=swiftshader",
    "--ignore-gpu-blocklist",
  );
} else {
  chromiumFlags.push("--disable-gpu", "--disable-software-rasterizer");
}

fs.mkdirSync("/chromium", { recursive: true });
fs.writeFileSync("/chromium/flags", JSON.stringify({ flags: chromiumFlags }));
