import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import * as esbuild from 'esbuild';
import ts from 'typescript';

const outfile = resolve('dist/trueos/index.js');

const pixiCaptureOnlyAlias = {
  name: 'pixi-capture-only-alias',
  setup(build) {
    build.onResolve({ filter: /^pixi\.js$/ }, () => ({
      path: resolve('src/pixiCaptureOnly.ts'),
    }));
    build.onResolve({ filter: /^yoga-layout$/ }, () => ({
      path: 'yoga-layout',
      namespace: 'trueos-empty-yoga',
    }));
    build.onLoad({ filter: /.*/, namespace: 'trueos-empty-yoga' }, () => ({
      contents: 'export default {};',
      loader: 'js',
    }));
  },
};

await mkdir(dirname(outfile), { recursive: true });

const esbuildResult = await esbuild.build({
  entryPoints: [resolve('src/main.ts')],
  bundle: true,
  format: 'iife',
  platform: 'browser',
  target: 'es2015',
  charset: 'ascii',
  minify: true,
  legalComments: 'none',
  define: {
    __TRUEOS_CAPTURE_BUILD__: 'true',
  },
  plugins: [pixiCaptureOnlyAlias],
  write: false,
});

const bundled = esbuildResult.outputFiles[0]?.text ?? '';
const downlevel = ts.transpileModule(bundled, {
  compilerOptions: {
    allowJs: true,
    target: ts.ScriptTarget.ES5,
    module: ts.ModuleKind.None,
    downlevelIteration: true,
    importHelpers: false,
    noEmitHelpers: false,
  },
}).outputText;

const ascii = downlevel.replace(/[^\x09\x0a\x0d\x20-\x7e]/g, (ch) => {
  const code = ch.charCodeAt(0);
  if (code <= 0xff) return `\\x${code.toString(16).padStart(2, '0').toUpperCase()}`;
  return `\\u${code.toString(16).padStart(4, '0').toUpperCase()}`;
});

for (const forbidden of ['\\\\s', '\\\\d', '\\\\S', '\\\\b']) {
  if (ascii.includes(forbidden)) {
    throw new Error(`TRUEOS QuickJS-hostile regexp escape still present: ${forbidden}`);
  }
}

await writeFile(outfile, ascii);
console.log(`trueos bundle ${outfile}`);
