import { defineConfig } from 'vite';
import { appendFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const pixiCommandLogPath = resolve(__dirname, 'pixi-commands.ndjson');
const trueosCaptureBuild = process.env.TRUEOS_CAPTURE_BUILD === '1';

export default defineConfig(({ command }) => {
  const isTrueosCaptureBuild = command === 'build' && trueosCaptureBuild;
  const buildTarget = isTrueosCaptureBuild ? 'es2015' : 'es2022';

  return {
  define: {
    __TRUEOS_CAPTURE_BUILD__: JSON.stringify(isTrueosCaptureBuild),
  },
  resolve: {
    alias:
      isTrueosCaptureBuild
        ? {
            'pixi.js': resolve(__dirname, 'src/pixiCaptureOnly.ts'),
            'yoga-layout': resolve(__dirname, 'src/trueosEmptyYoga.ts'),
          }
        : {},
  },
  plugins: [
    {
      name: 'pixi-command-capture-file',
      configureServer(server) {
        writeFileSync(pixiCommandLogPath, '');
        server.middlewares.use('/__pixi_capture', (req, res) => {
          if (req.method !== 'POST') {
            res.statusCode = 405;
            res.end('method not allowed');
            return;
          }

          let body = '';
          req.setEncoding('utf8');
          req.on('data', (chunk) => {
            body += chunk;
          });
          req.on('end', () => {
            if (body.length > 0) appendFileSync(pixiCommandLogPath, body.endsWith('\n') ? body : `${body}\n`);
            res.statusCode = 204;
            res.end();
          });
        });
      },
    },
  ],
  // Prevent stale index.html from being cached during `vite preview`, which can
  // otherwise make the browser request old hashed chunks after rebuilds.
  preview: {
    headers: {
      'Cache-Control': 'no-store',
    },
  },
  optimizeDeps: {
    esbuildOptions: {
      target: buildTarget,
    },
  },
  esbuild: {
    target: buildTarget,
  },
  build: {
    target: buildTarget,
    modulePreload: false,
    rollupOptions: {
      output: {
        entryFileNames: 'assets/[name].js',
        chunkFileNames: 'assets/[name].js',
        assetFileNames: 'assets/[name][extname]',
      },
    },
  },
  };
});
