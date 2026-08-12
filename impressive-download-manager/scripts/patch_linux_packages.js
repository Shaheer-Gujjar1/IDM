import fs from 'fs';
import path from 'path';
import { execSync } from 'child_process';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const rootDir = path.resolve(__dirname, '..');
const bundleDir = path.join(rootDir, 'src-tauri', 'target', 'release', 'bundle');
const debDir = path.join(bundleDir, 'deb');
const postrmSource = path.join(__dirname, 'postrm.sh');

export function patchDebPackages() {
  if (!fs.existsSync(debDir)) {
    console.log('[Post-Build] No deb directory found, skipping deb postrm patching.');
    return;
  }

  const files = fs.readdirSync(debDir);
  const debFiles = files.filter(f => f.endsWith('.deb'));

  if (debFiles.length === 0) {
    console.log('[Post-Build] No .deb files found to patch.');
    return;
  }

  const postrmContent = fs.readFileSync(postrmSource, 'utf-8');

  for (const debFile of debFiles) {
    const fullPath = path.join(debDir, debFile);
    console.log(`[Post-Build] 🛠️ Injecting postrm cleanup script into ${debFile}...`);

    const tempDir = path.join(debDir, `temp_${Date.now()}`);
    fs.mkdirSync(tempDir, { recursive: true });

    try {
      // 1. Unpack deb using dpkg-deb -R
      execSync(`dpkg-deb -R "${fullPath}" "${tempDir}"`, { stdio: 'inherit' });

      // 2. Write DEBIAN/postrm with executable permissions
      const debianDir = path.join(tempDir, 'DEBIAN');
      if (!fs.existsSync(debianDir)) {
        fs.mkdirSync(debianDir, { recursive: true });
      }
      const postrmPath = path.join(debianDir, 'postrm');
      fs.writeFileSync(postrmPath, postrmContent, { mode: 0o755 });

      // 3. Repack deb using dpkg-deb -b
      execSync(`dpkg-deb -b "${tempDir}" "${fullPath}"`, { stdio: 'inherit' });
      console.log(`[Post-Build] ✅ Successfully injected postrm into ${debFile}`);
    } catch (e) {
      console.error(`[Post-Build] ❌ Failed to patch ${debFile}:`, e);
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true });
    }
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  patchDebPackages();
}
