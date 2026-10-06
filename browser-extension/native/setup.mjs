import {
  readFileSync,
  mkdirSync,
  writeFileSync,
  existsSync,
  copyFileSync,
  chmodSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, resolve, join } from "node:path";
import { homedir } from "node:os";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const native = join(root, "browser-extension/native");
const id = readFileSync(
  join(root, "browser-extension/extension-id.txt"),
  "utf8",
).trim();
const quote = (s) => "'" + s.replaceAll("'", "'\\''") + "'";
mkdirSync(join(native, "bin"), { recursive: true });
const buildEnv = { ...process.env };
if (
  !buildEnv.DEVELOPER_DIR &&
  existsSync("/Applications/Xcode.app/Contents/Developer")
)
  buildEnv.DEVELOPER_DIR = "/Applications/Xcode.app/Contents/Developer";
const sdk = spawnSync(
  "xcrun",
  ["--no-cache", "--sdk", "macosx", "--show-sdk-path"],
  { encoding: "utf8", env: buildEnv },
);
if (sdk.status !== 0) throw Error(sdk.stderr);
const compiler = spawnSync("xcrun", ["--find", "swiftc"], {
  encoding: "utf8",
  env: buildEnv,
});
if (compiler.status !== 0) throw Error(compiler.stderr);
const result = spawnSync(
  compiler.stdout.trim(),
  [
    "-parse-as-library",
    join(root, "src-tauri/native/ScreenText.swift"),
    join(native, "ReaderHost.swift"),
    "-o",
    join(native, "bin/reader-host"),
  ],
  { stdio: "inherit", env: { ...buildEnv, SDKROOT: sdk.stdout.trim() } },
);
if (result.status !== 0) process.exit(result.status || 1);
const launcher = join(native, "bin/launch-host");
writeFileSync(
  launcher,
  `#!/bin/sh\nexec ${quote(join(native, "bin/reader-host"))} "$@"\n`,
  { mode: 0o700 },
);
const manifest = {
  name: "com.texthalo.reader",
  description: "TextHalo browser reader relay",
  path: launcher,
  type: "stdio",
  allowed_origins: [`chrome-extension://${id}/`],
};
writeFileSync(
  join(native, "bin/com.texthalo.reader.json"),
  JSON.stringify(manifest, null, 2) + "\n",
);
if (process.argv.includes("--install")) {
  const installed = join(
    homedir(),
    "Library/Application Support/TextHalo Reader",
  );
  mkdirSync(installed, { recursive: true, mode: 0o700 });
  copyFileSync(join(native, "bin/reader-host"), join(installed, "reader-host"));
  chmodSync(join(installed, "reader-host"), 0o700);
  const installedLauncher = join(installed, "launch-host");
  writeFileSync(
    installedLauncher,
    `#!/bin/sh\nexec ${quote(join(installed, "reader-host"))} "$@"\n`,
    { mode: 0o700 },
  );
  manifest.path = installedLauncher;
  const dir = join(
    homedir(),
    "Library/Application Support/Google/Chrome/NativeMessagingHosts",
  );
  mkdirSync(dir, { recursive: true });
  const target = join(dir, "com.texthalo.reader.json");
  if (existsSync(target)) {
    const previous = JSON.parse(readFileSync(target, "utf8"));
    if (
      previous.name !== manifest.name ||
      previous.path !== manifest.path ||
      previous.type !== manifest.type ||
      JSON.stringify(previous.allowed_origins) !==
        JSON.stringify(manifest.allowed_origins)
    )
      throw Error(
        "Existing native host registration differs; refusing to replace it.",
      );
  }
  writeFileSync(target, JSON.stringify(manifest, null, 2) + "\n", {
    mode: 0o600,
  });
  console.log("Registered local OCR host:", target);
} else
  console.log(
    "Built local OCR host. Run with --install to register it for Chrome.",
  );
