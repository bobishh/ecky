import { delimiter, dirname, join } from "node:path";
import { spawn, spawnSync } from "node:child_process";

const commandWorks = (command, args) => {
  const result = spawnSync(command, args, { stdio: "ignore" });
  return result.status === 0;
};

const rustupOutput = (args) => {
  const result = spawnSync("rustup", args, { encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : null;
};

const nativeStableToolchain = () => {
  const target =
    process.platform === "darwin"
      ? `${process.arch === "arm64" ? "aarch64" : "x86_64"}-apple-darwin`
      : process.platform === "linux"
        ? `${process.arch === "arm64" ? "aarch64" : "x86_64"}-unknown-linux-gnu`
        : process.platform === "win32"
          ? `${process.arch === "arm64" ? "aarch64" : "x86_64"}-pc-windows-msvc`
          : null;
  if (!target) return null;

  const installed = rustupOutput(["toolchain", "list"]);
  if (!installed) return null;
  const wanted = `stable-${target}`;
  return installed
    .split(/\r?\n/)
    .map((line) => line.split(/\s+/)[0])
    .find((toolchain) => toolchain === wanted) ?? null;
};

const env = { ...process.env };
if (!commandWorks("rustc", ["--version"])) {
  const toolchain = nativeStableToolchain();
  if (!toolchain) {
    console.error(
      "Rust compiler is unavailable and no native rustup stable toolchain is installed.",
    );
    process.exit(1);
  }

  const cargo = rustupOutput(["which", "--toolchain", toolchain, "cargo"]);
  const rustc = rustupOutput(["which", "--toolchain", toolchain, "rustc"]);
  if (!cargo || !rustc) {
    console.error(`Could not resolve cargo/rustc for ${toolchain}.`);
    process.exit(1);
  }

  env.CARGO = cargo;
  env.RUSTC = rustc;
  env.PATH = [dirname(cargo), dirname(rustc), env.PATH ?? ""].join(delimiter);
  console.error(`[tauri] broken system rustc; using ${toolchain}`);
}

const executable = join(
  process.cwd(),
  "node_modules",
  ".bin",
  process.platform === "win32" ? "tauri.cmd" : "tauri",
);
const child = spawn(executable, process.argv.slice(2), {
  env,
  stdio: "inherit",
});

child.on("error", (error) => {
  console.error(`Failed to start Tauri CLI: ${error.message}`);
  process.exit(1);
});
child.on("exit", (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  else process.exit(code ?? 1);
});
