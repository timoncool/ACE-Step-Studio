import fs from 'fs';
import path from 'path';
import { defineConfig } from 'vitest/config';

const studio = JSON.parse(fs.readFileSync(path.resolve(__dirname, '../studio.json'), 'utf-8'));

export default defineConfig({
  define: { __STUDIO__: JSON.stringify(studio), __APP_VERSION__: JSON.stringify('test') },
  test: { environment: 'happy-dom', globals: true, include: ['**/*.test.ts', '**/*.test.tsx'], passWithNoTests: true },
});
