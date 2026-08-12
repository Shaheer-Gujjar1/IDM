import { execSync } from 'child_process';
import fs from 'fs';
import os from 'os';
import path from 'path';

const homeDir = os.homedir();
const keyPath = path.join(homeDir, '.tauri', 'idm.key');

if (fs.existsSync(keyPath)) {
  const privateKey = fs.readFileSync(keyPath, 'utf-8').trim();
  process.env.TAURI_SIGNING_PRIVATE_KEY = privateKey;
  console.log('🔑 Loaded Tauri signing private key from:', keyPath);
} else {
  console.warn('⚠️ Warning: Private key file not found at:', keyPath);
  console.warn('   The build will proceed, but updates will not be signed.');
}

console.log('🚀 Executing Tauri release build...');
execSync('npx tauri build', {
  stdio: 'inherit',
  env: process.env
});

console.log('📝 Generating latest.json release manifest...');
execSync('node scripts/generate_latest_json.js', {
  stdio: 'inherit',
  env: process.env
});
