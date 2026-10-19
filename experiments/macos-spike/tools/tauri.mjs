import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

if (process.platform !== "darwin") {
  console.error(
    "Mac 原生程序需要在 macOS 上构建或运行。Windows 可使用 npm run macos:preview 和 npm run macos:check。",
  );
  process.exit(1);
}

const [command, ...args] = process.argv.slice(2);
if (command !== "dev" && command !== "build") {
  console.error("Usage: node tools/tauri.mjs <dev|build> [Tauri arguments]");
  process.exit(2);
}

const require = createRequire(import.meta.url);
const result = spawnSync(
  process.execPath,
  [require.resolve("@tauri-apps/cli/tauri.js"), command, ...args],
  { cwd: fileURLToPath(new URL("..", import.meta.url)), stdio: "inherit" },
);
if (result.error) console.error(result.error.message);
process.exitCode = result.status ?? 1;
