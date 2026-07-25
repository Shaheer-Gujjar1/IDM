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

// Clean latest.json manifest output
const outPath = path.join(bundleDir, 'latest.json');
let latestJson = {
  version: version,
  notes: `Release ${tag} of Impressive Download Manager`,
  pub_date: new Date().toISOString(),
  platforms: {}
};

// 1. Linux Standard Target (AppImage for native in-place updates)
const appimageDir = path.join(bundleDir, 'appimage');
if (fs.existsSync(appimageDir)) {
  const files = fs.readdirSync(appimageDir);
  const file = files.find(f => f.endsWith('.AppImage'));
  const sig = files.find(f => f.endsWith('.AppImage.sig'));
  if (file && sig) {
    const signature = fs.readFileSync(path.join(appimageDir, sig), 'utf-8').trim();
    latestJson.platforms['linux-x86_64'] = {
      signature,
      url: `${baseUrl}/${file}`
    };
  }
}

// 2. Linux DEB Target
const debDir = path.join(bundleDir, 'deb');
if (fs.existsSync(debDir)) {
  const files = fs.readdirSync(debDir);
  const file = files.find(f => f.endsWith('.deb'));
  const sig = files.find(f => f.endsWith('.deb.sig'));
  if (file && sig) {
    const signature = fs.readFileSync(path.join(debDir, sig), 'utf-8').trim();
    // Also attach to linux-x86_64 if AppImage is not built
    if (!latestJson.platforms['linux-x86_64']) {
      latestJson.platforms['linux-x86_64'] = {
        signature,
        url: `${baseUrl}/${file}`
      };
    }
    latestJson.platforms['linux-x86_64-deb'] = {
      signature,
      url: `${baseUrl}/${file}`
    };
  }
}

// 3. Linux RPM Target
const rpmDir = path.join(bundleDir, 'rpm');
if (fs.existsSync(rpmDir)) {
  const files = fs.readdirSync(rpmDir);
  const file = files.find(f => f.endsWith('.rpm'));
  const sig = files.find(f => f.endsWith('.rpm.sig'));
  if (file && sig) {
    const signature = fs.readFileSync(path.join(rpmDir, sig), 'utf-8').trim();
    latestJson.platforms['linux-x86_64-rpm'] = {
      signature,
      url: `${baseUrl}/${file}`
    };
  }
}

// 4. Windows 64-bit Target
const nsisDir = path.join(bundleDir, 'nsis');
const msiDir = path.join(bundleDir, 'msi');
if (fs.existsSync(nsisDir)) {
  const files = fs.readdirSync(nsisDir);
  const file = files.find(f => f.endsWith('.exe'));
  const sig = files.find(f => f.endsWith('.exe.sig'));
  if (file && sig) {
    const signature = fs.readFileSync(path.join(nsisDir, sig), 'utf-8').trim();
    latestJson.platforms['windows-x86_64'] = {
      signature,
      url: `${baseUrl}/${file}`
    };
  }
} else if (fs.existsSync(msiDir)) {
  const files = fs.readdirSync(msiDir);
  const file = files.find(f => f.endsWith('.msi'));
  const sig = files.find(f => f.endsWith('.msi.sig'));
  if (file && sig) {
    const signature = fs.readFileSync(path.join(msiDir, sig), 'utf-8').trim();
    latestJson.platforms['windows-x86_64'] = {
      signature,
      url: `${baseUrl}/${file}`
    };
  }
}

// 5. macOS Targets
const dmgDir = path.join(bundleDir, 'dmg');
const macosDir = path.join(bundleDir, 'macos');
if (fs.existsSync(dmgDir) || fs.existsSync(macosDir)) {
  const targetDir = fs.existsSync(dmgDir) ? dmgDir : macosDir;
  const files = fs.readdirSync(targetDir);
  const file = files.find(f => f.endsWith('.app.tar.gz') || f.endsWith('.dmg'));
  const sig = files.find(f => f.endsWith('.sig'));
  if (file && sig) {
    const signature = fs.readFileSync(path.join(targetDir, sig), 'utf-8').trim();
    latestJson.platforms['darwin-x86_64'] = {
      signature,
      url: `${baseUrl}/${file}`
    };
    latestJson.platforms['darwin-aarch64'] = {
      signature,
      url: `${baseUrl}/${file}`
    };
  }
}

fs.writeFileSync(outPath, JSON.stringify(latestJson, null, 2));

console.log(`\n✅ Generated latest.json for ${tag}:`);
console.log(outPath);
console.log(JSON.stringify(latestJson, null, 2));
