import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
const pkg = JSON.parse(read('package.json'));
const lock = JSON.parse(read('package-lock.json'));
const config = JSON.parse(read('src-tauri/tauri.conf.json'));
for (const icon of ['icons/128x128.png', 'icons/icon.ico', 'icons/icon.icns']) {
  assert.ok(config.bundle.icon?.includes(icon), `Missing bundle icon: ${icon}`);
}
for (const icon of config.bundle.icon) {
  assert.ok(existsSync(new URL(`../src-tauri/${icon}`, import.meta.url)), `Bundle icon does not exist: ${icon}`);
}
const cargo = read('src-tauri/Cargo.toml').split(/^\[/m).find((section) => section.startsWith('package]'));
const rustVersion = cargo?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const cargoLock = read('src-tauri/Cargo.lock').split('[[package]]')
  .find((section) => /^name = "QVault"$/m.test(section));
const lockedRustVersion = cargoLock?.match(/^version = "([^"]+)"/m)?.[1];

for (const [name, version] of Object.entries({
  'package-lock.json': lock.version,
  'package-lock.json root': lock.packages[''].version,
  'tauri.conf.json': config.version,
  'Cargo.toml': rustVersion,
  'Cargo.lock': lockedRustVersion,
})) {
  assert.equal(version, pkg.version, `${name} version must match package.json (${pkg.version})`);
}

if (process.env.GITHUB_REF_TYPE === 'tag') {
  assert.equal(process.env.GITHUB_REF_NAME, `v${pkg.version}`, 'Release tag must match the application version');
}
console.log(`Release version verified: ${pkg.version}`);
