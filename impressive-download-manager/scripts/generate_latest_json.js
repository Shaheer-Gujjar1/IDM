import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const rootDir = path.resolve(__dirname, '..');
const pkgPath = path.join(rootDir, 'package.json');
const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf-8'));
const version = pkg.version;
const tag = `v${version}`;

const bundleDir = path.join(rootDir, 'src-tauri', 'target', 'release', 'bundle');
const baseUrl = `https://github.com/Shaheer-Gujjar1/IDM/releases/download/${tag}`;

// Clean latest.json manifest output (DEB for linux-x86_64 & linux-x86_64-deb, RPM for linux-x86_64-rpm, EXE for windows-x86_64)
const outPath = path.join(bundleDir, 'latest.json');
let latestJson = {
  version: version,
  notes: `Release ${tag} of Impressive Download Manager`,
  pub_date: new Date().toISOString(),
  platforms: {}
};

// Helper: Get newest file matching extension & version
const findPackageFile = (dir, ext) => {
  if (!fs.existsSync(dir)) return null;
  const files = fs.readdirSync(dir);
  // First attempt: match current version string
  let target = files.find(f => f.includes(version) && f.endsWith(ext) && !f.endsWith('.sig'));
  // Fallback: match any file ending with extension
  if (!target) {
    target = files.find(f => f.endsWith(ext) && !f.endsWith('.sig'));
  }
  if (!target) return null;

  // Look for matching .sig file
  const sigFile = files.find(f => f.endsWith('.sig') || f.includes('.sig'));
  if (!sigFile) return null;

  const signature = fs.readFileSync(path.join(dir, sigFile), 'utf-8').trim();
  return { file: target, signature };
};

// 1. Linux DEB Target (Debian / Ubuntu / Deepin / Mint) -> 'linux-x86_64-deb' & 'linux-x86_64'
const debDir = path.join(bundleDir, 'deb');
const debMatch = findPackageFile(debDir, '.deb');
if (debMatch) {
  latestJson.platforms['linux-x86_64-deb'] = {
    signature: debMatch.signature,
    url: `${baseUrl}/${debMatch.file}`
  };
  latestJson.platforms['linux-x86_64'] = {
    signature: debMatch.signature,
    url: `${baseUrl}/${debMatch.file}`
  };
}

// 2. Linux RPM Target (Fedora / RHEL / CentOS / openSUSE) -> 'linux-x86_64-rpm'
const rpmDir = path.join(bundleDir, 'rpm');
const rpmMatch = findPackageFile(rpmDir, '.rpm');
if (rpmMatch) {
  latestJson.platforms['linux-x86_64-rpm'] = {
    signature: rpmMatch.signature,
    url: `${baseUrl}/${rpmMatch.file}`
  };
}

// 3. Windows 64-bit NSIS / MSI Target -> 'windows-x86_64'
const nsisDir = path.join(bundleDir, 'nsis');
const msiDir = path.join(bundleDir, 'msi');
const winMatch = findPackageFile(nsisDir, '.exe') || findPackageFile(msiDir, '.msi');
if (winMatch) {
  latestJson.platforms['windows-x86_64'] = {
    signature: winMatch.signature,
    url: `${baseUrl}/${winMatch.file}`
  };
}

// 4. macOS Target (.app.tar.gz or .dmg) -> 'darwin-x86_64' / 'darwin-aarch64'
const dmgDir = path.join(bundleDir, 'dmg');
const macosDir = path.join(bundleDir, 'macos');
const macMatch = findPackageFile(dmgDir, '.dmg') || findPackageFile(macosDir, '.app.tar.gz');
if (macMatch) {
  latestJson.platforms['darwin-x86_64'] = {
    signature: macMatch.signature,
    url: `${baseUrl}/${macMatch.file}`
  };
  latestJson.platforms['darwin-aarch64'] = {
    signature: macMatch.signature,
    url: `${baseUrl}/${macMatch.file}`
  };
}

// Write generated manifest to bundleDir
fs.writeFileSync(outPath, JSON.stringify(latestJson, null, 2));

console.log(`\n✅ Successfully generated latest.json manifest for ${tag}:`);
console.log(outPath);
console.log(JSON.stringify(latestJson, null, 2));
