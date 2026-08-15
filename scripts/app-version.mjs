import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const paths = {
  packageJson: path.join(root, "package.json"),
  packageLock: path.join(root, "package-lock.json"),
  cargoToml: path.join(root, "src-tauri", "Cargo.toml"),
  cargoLock: path.join(root, "src-tauri", "Cargo.lock"),
  tauriConfig: path.join(root, "src-tauri", "tauri.conf.json"),
};

const semverPattern =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function writeJson(file, value) {
  fs.writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);
}

function replaceRequired(file, pattern, replacement) {
  const value = fs.readFileSync(file, "utf8");
  const flags = pattern.flags.includes("g") ? pattern.flags : `${pattern.flags}g`;
  const matches = value.match(new RegExp(pattern.source, flags));

  if (matches?.length !== 1) {
    throw new Error(
      `Expected exactly one application version in ${path.relative(root, file)}`,
    );
  }

  fs.writeFileSync(file, value.replace(pattern, replacement));
}

function assertVersion(version, label) {
  if (!semverPattern.test(version)) {
    throw new Error(`${label} is not a valid semantic version: ${version}`);
  }
}

function versions() {
  const packageJson = readJson(paths.packageJson);
  const packageLock = readJson(paths.packageLock);
  const tauriConfig = readJson(paths.tauriConfig);
  const cargoToml = fs.readFileSync(paths.cargoToml, "utf8");
  const cargoLock = fs.readFileSync(paths.cargoLock, "utf8");
  const cargoTomlMatch = cargoToml.match(/^\[package\][\s\S]*?^version = "([^"]+)"$/m);
  const cargoLockMatch = cargoLock.match(
    /\[\[package\]\]\r?\nname = "wakatoken"\r?\nversion = "([^"]+)"/,
  );

  if (!cargoTomlMatch || !cargoLockMatch) {
    throw new Error("Could not read the WakaToken Cargo package version");
  }

  return {
    canonical: packageJson.version,
    packageLock: packageLock.version,
    packageLockRoot: packageLock.packages?.[""]?.version,
    cargoToml: cargoTomlMatch[1],
    cargoLock: cargoLockMatch[1],
    tauriConfig: tauriConfig.version,
  };
}

function check(expectedVersion) {
  const current = versions();
  assertVersion(current.canonical, "package.json version");

  const expected = {
    packageLock: current.canonical,
    packageLockRoot: current.canonical,
    cargoToml: current.canonical,
    cargoLock: current.canonical,
    tauriConfig: "../package.json",
  };

  for (const [name, value] of Object.entries(expected)) {
    if (current[name] !== value) {
      throw new Error(`${name} is ${current[name]}, expected ${value}`);
    }
  }

  if (expectedVersion !== undefined && current.canonical !== expectedVersion) {
    throw new Error(
      `Committed version ${current.canonical} does not match release tag ${expectedVersion}`,
    );
  }

  console.log(`Application version ${current.canonical} is consistent`);
}

function set(version) {
  assertVersion(version, "Requested version");

  const packageJson = readJson(paths.packageJson);
  packageJson.version = version;
  writeJson(paths.packageJson, packageJson);

  const packageLock = readJson(paths.packageLock);
  packageLock.version = version;
  packageLock.packages[""].version = version;
  writeJson(paths.packageLock, packageLock);

  replaceRequired(
    paths.cargoToml,
    /(^\[package\][\s\S]*?^version = ")[^"]+("$)/m,
    `$1${version}$2`,
  );
  replaceRequired(
    paths.cargoLock,
    /(\[\[package\]\]\r?\nname = "wakatoken"\r?\nversion = ")[^"]+(")/,
    `$1${version}$2`,
  );

  check(version);
}

const [command, version] = process.argv.slice(2);

if (command === "check") {
  check(version);
} else if (command === "set" && version !== undefined) {
  set(version);
} else {
  throw new Error("Usage: node scripts/app-version.mjs check [version] | set <version>");
}
