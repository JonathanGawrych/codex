#!/Applications/ChatGPT.app/Contents/Resources/cua_node/bin/node

import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile, rename, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const proxyPath = fileURLToPath(import.meta.url);
const chromeManifestPath =
  `${process.env.HOME}/Library/Application Support/Google/Chrome/NativeMessagingHosts/com.openai.codexextension.json`;
const nativeHostPath =
  `${process.env.HOME}/.codex/plugins/cache/openai-bundled/chrome/latest/extension-host/macos/arm64/ChatGPT for Chrome`;
const browserClientPath =
  `${process.env.HOME}/.codex/plugins/cache/openai-bundled/chrome/latest/scripts/browser-client.mjs`;
const nodeModuleDirs = [
  "/Applications/ChatGPT.app/Contents/Resources/cua_node/lib/node_modules",
];
const trustedBrowserClientSha256s = [
  createHash("sha256").update(await readFile(browserClientPath)).digest("hex"),
];

if (process.argv[2] === "--install") {
  const manifest = JSON.parse(await readFile(chromeManifestPath, "utf8"));
  manifest.path = proxyPath;
  const temporaryManifestPath =
    `${chromeManifestPath}.source-install-${process.pid}`;
  await writeFile(
    temporaryManifestPath,
    `${JSON.stringify(manifest, null, 2)}\n`,
  );
  await rename(temporaryManifestPath, chromeManifestPath);
  process.exit(0);
}

const nativeHost = spawn(nativeHostPath, process.argv.slice(2), {
  stdio: ["pipe", "pipe", "pipe"],
});

process.stdin.pipe(nativeHost.stdin);
nativeHost.stderr.pipe(process.stderr);

let nativeHostOutput = Buffer.alloc(0);

nativeHost.stdout.on("data", (chunk) => {
  nativeHostOutput = Buffer.concat([nativeHostOutput, chunk]);
  while (nativeHostOutput.length >= 4) {
    const messageLength = nativeHostOutput.readUInt32LE(0);
    if (nativeHostOutput.length < messageLength + 4) {
      return;
    }

    const message = JSON.parse(
      nativeHostOutput.subarray(4, messageLength + 4).toString("utf8"),
    );
    nativeHostOutput = nativeHostOutput.subarray(messageLength + 4);

    if (message.result?.runtimeConfig != null) {
      message.result.runtimeConfig.nodeModuleDirs ??= nodeModuleDirs;
      message.result.runtimeConfig.trustedBrowserClientSha256s ??=
        trustedBrowserClientSha256s;
    }

    const encodedMessage = Buffer.from(JSON.stringify(message));
    const header = Buffer.alloc(4);
    header.writeUInt32LE(encodedMessage.length, 0);
    process.stdout.write(Buffer.concat([header, encodedMessage]));
  }
});

nativeHost.on("error", (error) => {
  console.error(error);
  process.exitCode = 1;
});

nativeHost.on("exit", (code, signal) => {
  if (signal != null) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});
