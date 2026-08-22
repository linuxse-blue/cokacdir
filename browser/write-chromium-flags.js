const fs = require("fs");

const width = process.env.WIDTH || process.env.BROWSER_WIDTH || "1920";
const height = process.env.HEIGHT || process.env.BROWSER_HEIGHT || "1080";

const chromiumFlags = [
  "--user-data-dir=/home/kernel/user-data",
  "--disable-dev-shm-usage",
  `--window-size=${width},${height}`,
  "--window-position=0,0",
  "--start-maximized",
  "--remote-allow-origins=*",
  "--disable-infobars",
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

