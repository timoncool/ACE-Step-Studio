import fs from 'fs';
import path from 'path';
import { defineConfig, loadEnv } from 'vite';
import react from '@vitejs/plugin-react';

const studio = JSON.parse(fs.readFileSync(path.resolve(__dirname, '../studio.json'), 'utf-8'));
const service = `http://127.0.0.1:${studio.port}`;
const appVersion = JSON.parse(fs.readFileSync(path.resolve(__dirname, 'package.json'), 'utf-8')).version;

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, '.', '');
  return {
    server: {
      port: 3000,
      host: '0.0.0.0',
      proxy: {
        '/v1': {
          target: service,
          changeOrigin: true,
        },
        '/setup': {
          target: service,
          changeOrigin: true,
        },
        '/engine': {
          target: service,
          changeOrigin: true,
        },
        '/health': {
          target: service,
          changeOrigin: true,
        },
        // There is deliberately no proxy for the retired ACE Node service:
        // the studio talks to the native Rust server only, so a stray legacy
        // request fails loudly in development instead of silently 500-ing.
      },
    },
    optimizeDeps: {
      exclude: ['@ffmpeg/ffmpeg', '@ffmpeg/util'],
    },
    define: {
      __STUDIO__: JSON.stringify(studio),
      __APP_VERSION__: JSON.stringify(appVersion),
    },
    plugins: [react()],
    resolve: {
      alias: {
        '@': path.resolve(__dirname, '.'),
      }
    }
  };
});
