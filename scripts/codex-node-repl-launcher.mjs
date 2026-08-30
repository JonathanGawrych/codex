#!/Applications/ChatGPT.app/Contents/Resources/cua_node/bin/node

import { spawn } from "node:child_process";
import { constants } from "node:fs";
import { access, readFile, rename, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const chatGptNodePath =
  "/Applications/ChatGPT.app/Contents/Resources/cua_node/bin/node";
const nodeReplPath =
  "/Applications/ChatGPT.app/Contents/Resources/cua_node/bin/node_repl";
const launcherPath = fileURLToPath(import.meta.url);

if (process.argv[2] === "--install") {
  await installLauncherConfiguration();
  process.exit(0);
}

const nodeRepl = spawn(nodeReplPath, process.argv.slice(2), {
  env: process.env,
  stdio: "inherit",
});

const forwardedSignals = ["SIGINT", "SIGHUP", "SIGTERM"];
const signalHandlers = new Map(
  forwardedSignals.map((signal) => [signal, () => nodeRepl.kill(signal)]),
);
for (const [signal, handler] of signalHandlers) {
  process.on(signal, handler);
}

nodeRepl.on("error", (error) => {
  console.error(`Failed to start ${nodeReplPath}:`, error);
  process.exit(1);
});

nodeRepl.on("exit", (code, signal) => {
  for (const [forwardedSignal, handler] of signalHandlers) {
    process.off(forwardedSignal, handler);
  }
  if (signal != null) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});

async function installLauncherConfiguration() {
  await access(chatGptNodePath, constants.X_OK);
  await access(nodeReplPath, constants.X_OK);

  const codexHome = process.env.CODEX_HOME ?? path.join(requiredHome(), ".codex");
  const configPath = path.join(codexHome, "config.toml");
  const config = await readFile(configPath, "utf8");
  const updatedConfig = configureNodeRepl(config);
  if (updatedConfig === config) {
    return;
  }

  const configStat = await stat(configPath);
  const temporaryConfigPath = `${configPath}.source-install-${process.pid}`;
  await writeFile(temporaryConfigPath, updatedConfig, {
    mode: configStat.mode,
  });
  await rename(temporaryConfigPath, configPath);
}

function requiredHome() {
  if (process.env.HOME == null || process.env.HOME === "") {
    throw new Error("HOME must be set when CODEX_HOME is not set");
  }
  return process.env.HOME;
}

function configureNodeRepl(config) {
  const eol = config.includes("\r\n") ? "\r\n" : "\n";
  const endsWithEol = config.endsWith(eol);
  const lines = config.split(/\r?\n/);
  if (endsWithEol) {
    lines.pop();
  }

  const tableHeader = "[mcp_servers.node_repl]";
  const tableIndexes = lines.flatMap((line, index) =>
    line.trim() === tableHeader ? [index] : [],
  );
  if (tableIndexes.length !== 1) {
    throw new Error(
      `Expected exactly one ${tableHeader} table in the Codex config`,
    );
  }

  const tableStart = tableIndexes[0];
  const nextTableOffset = lines
    .slice(tableStart + 1)
    .findIndex((line) => line.trimStart().startsWith("["));
  const tableEnd =
    nextTableOffset === -1 ? lines.length : tableStart + 1 + nextTableOffset;
  const keyIndexes = new Map([
    ["args", []],
    ["command", []],
  ]);
  for (let index = tableStart + 1; index < tableEnd; index += 1) {
    const keyMatch = lines[index].match(/^\s*(args|command)\s*=/);
    if (keyMatch != null) {
      keyIndexes.get(keyMatch[1]).push(index);
    }
  }

  for (const [key, indexes] of keyIndexes) {
    if (indexes.length > 1) {
      throw new Error(`Found duplicate ${key} keys in ${tableHeader}`);
    }
  }

  const desiredValues = new Map([
    ["args", `args = [${JSON.stringify(launcherPath)}]`],
    ["command", `command = ${JSON.stringify(chatGptNodePath)}`],
  ]);
  for (const [key, desiredValue] of desiredValues) {
    const indexes = keyIndexes.get(key);
    if (indexes.length === 1) {
      lines[indexes[0]] = desiredValue;
    }
  }
  const missingValues = [...desiredValues]
    .filter(([key]) => keyIndexes.get(key).length === 0)
    .map(([, desiredValue]) => desiredValue);
  lines.splice(tableStart + 1, 0, ...missingValues);

  return `${lines.join(eol)}${endsWithEol ? eol : ""}`;
}
