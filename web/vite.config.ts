import { defineConfig, type Plugin } from 'vite';
import react from '@vitejs/plugin-react';

/**
 * Drop the parts of `animal-island-ui`'s bundle that this app does not use.
 *
 * The library ships six Noto Sans SC faces — three Simplified-Chinese ones at
 * ~1.15 MB each, declared without a `unicode-range`, so a browser is free to
 * pull all three — plus four decorative scene illustrations.
 *
 * - Typography: every `font-family` list that names "Noto Sans SC" continues
 *   with the platform CJK fonts (`PingFang SC` / `Microsoft YaHei`), so simply
 *   removing the faces hands Chinese text to the system font and nothing else
 *   changes. Nunito (≈50 KB) stays: it is the rounded Latin face the island
 *   style is built on.
 * - Scene images: only `Background type="sweet-corner" | "coffee-break"` and
 *   the `Progress variant="…"` fills use them. This app uses neither, and they
 *   are imported by the library's own JS, so the emitted files have to go.
 *   Start using those variants again and the matching entry below is what to
 *   delete.
 */
const NOTO_FONT_FACE = /@font-face\{[^{}]*Noto Sans SC[^{}]*\}/g;
const SCENE_IMAGE = /(?:^|[/\\])(?:sweet-corner|coffee-break|forest-grove|starry-camp)\.[^/\\]*\.svg$/;

function trimIslandAssets(): Plugin {
  let droppedImages = 0;
  let warned = false;

  return {
    name: 'trim-island-assets',
    // Ahead of Vite's own CSS plugin, so the stripped URLs are never resolved.
    enforce: 'pre',
    transform(code, id) {
      if (!id.includes('animal-island-ui') || !id.endsWith('.css')) return null;
      // Only the package's aggregated sheet declares fonts; per-component sheets
      // are left alone rather than re-scanning (and warning about) each of them.
      if (!code.includes('@font-face')) return null;

      const trimmed = code.replace(NOTO_FONT_FACE, '');
      if (trimmed === code) {
        if (!warned) {
          warned = true;
          this.warn(
            'trim-island-assets: no Noto Sans SC @font-face found — check the library version.',
          );
        }
        return null;
      }
      return { code: trimmed, map: null };
    },
    generateBundle(_options, bundle) {
      for (const fileName of Object.keys(bundle)) {
        if (SCENE_IMAGE.test(fileName)) {
          delete bundle[fileName];
          droppedImages += 1;
        }
      }
      if (droppedImages === 0) {
        this.warn('trim-island-assets: no scene images matched — check the library version.');
      }
    },
  };
}

// The WASM bindings are generated into `src/wasm` by `npm run build:wasm`.
// Vite understands the `new URL('nes_wasm_bg.wasm', import.meta.url)` pattern
// the generated bindings use, so the binary is copied to the output for free.
export default defineConfig({
  plugins: [react(), trimIslandAssets()],
  server: {
    port: 5173,
    host: true,
  },
  build: {
    target: 'es2022',
    outDir: 'dist',
  },
});
