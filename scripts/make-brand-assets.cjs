#!/usr/bin/env node
// Builds every raster brand asset from the SVG sources in assets/brand/:
//   src-tauri/icons/icon.ico            multi-size: 16 20 24 32 40 48 from icon-2-small.svg, 64 128 256 from icon-2.svg
//   src-tauri/icons/icon.png            512 x 512 from icon-2.svg
//   center/CompanyIsland.Center/Assets/icon.ico   the same .ico (the Center's exe icon and window icon)
//   src-tauri/nsis/installer-header.bmp   150 x 57, 24-bit, light (wordmark)
//   src-tauri/nsis/installer-sidebar.bmp  164 x 314, 24-bit, dark ink (icon + wordmark)
//
// Usage:  node scripts/make-brand-assets.cjs [--preview <dir>]
// It rasterises with Chromium (Playwright is a dev dependency). Set CHROME_PATH to use a specific chrome.exe.
// --preview writes PNG copies of every generated image (and each .ico entry) to <dir> for a visual check.
const fs = require('fs');
const path = require('path');

const ROOT = path.join(__dirname, '..');
const BRAND = path.join(ROOT, 'assets', 'brand');
const ICONS = path.join(ROOT, 'src-tauri', 'icons');
const NSIS = path.join(ROOT, 'src-tauri', 'nsis');
const CENTER_ICON = path.join(ROOT, 'center', 'CompanyIsland.Center', 'Assets', 'icon.ico');

const SMALL_SIZES = [16, 20, 24, 32, 40, 48];
const LARGE_SIZES = [64, 128, 256];

const previewIdx = process.argv.indexOf('--preview');
const previewDir = previewIdx > 0 ? path.resolve(process.argv[previewIdx + 1]) : null;

const uri = (name) => 'data:image/svg+xml;base64,' + fs.readFileSync(path.join(BRAND, name)).toString('base64');

// ---------- ICO / BMP writers ----------
function dibEntry(rgba, size) {
  // 32-bit BGRA, bottom-up, straight alpha, plus a 1-bpp AND mask (all zero: transparency comes from alpha).
  const header = Buffer.alloc(40);
  header.writeUInt32LE(40, 0);
  header.writeInt32LE(size, 4);
  header.writeInt32LE(size * 2, 8);
  header.writeUInt16LE(1, 12);
  header.writeUInt16LE(32, 14);
  const pixels = Buffer.alloc(size * size * 4);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const s = ((size - 1 - y) * size + x) * 4;
      const d = (y * size + x) * 4;
      pixels[d] = rgba[s + 2];
      pixels[d + 1] = rgba[s + 1];
      pixels[d + 2] = rgba[s];
      pixels[d + 3] = rgba[s + 3];
    }
  }
  const maskRow = Math.ceil(size / 32) * 4;
  const mask = Buffer.alloc(maskRow * size);
  header.writeUInt32LE(pixels.length + mask.length, 20);
  return Buffer.concat([header, pixels, mask]);
}

function buildIco(entries) {
  // entries: [{ size, data }] sorted as given
  const head = Buffer.alloc(6);
  head.writeUInt16LE(0, 0);
  head.writeUInt16LE(1, 2);
  head.writeUInt16LE(entries.length, 4);
  let offset = 6 + 16 * entries.length;
  const dir = [];
  for (const e of entries) {
    const d = Buffer.alloc(16);
    d[0] = e.size >= 256 ? 0 : e.size;
    d[1] = e.size >= 256 ? 0 : e.size;
    d.writeUInt16LE(1, 4);
    d.writeUInt16LE(32, 6);
    d.writeUInt32LE(e.data.length, 8);
    d.writeUInt32LE(offset, 12);
    offset += e.data.length;
    dir.push(d);
  }
  return Buffer.concat([head, ...dir, ...entries.map((e) => e.data)]);
}

function bmp24(rgb, w, h) {
  // NSIS wants a plain Windows BMP: 24-bit, uncompressed, bottom-up, rows padded to 4 bytes.
  const rowSize = Math.ceil((w * 3) / 4) * 4;
  const out = Buffer.alloc(14 + 40 + rowSize * h);
  out.write('BM', 0);
  out.writeUInt32LE(out.length, 2);
  out.writeUInt32LE(54, 10);
  out.writeUInt32LE(40, 14);
  out.writeInt32LE(w, 18);
  out.writeInt32LE(h, 22);
  out.writeUInt16LE(1, 26);
  out.writeUInt16LE(24, 28);
  out.writeUInt32LE(rowSize * h, 34);
  out.writeInt32LE(2835, 38);
  out.writeInt32LE(2835, 42);
  for (let y = 0; y < h; y++) {
    const dst = 54 + (h - 1 - y) * rowSize;
    for (let x = 0; x < w; x++) {
      const s = (y * w + x) * 4;
      out[dst + x * 3] = rgb[s + 2];
      out[dst + x * 3 + 1] = rgb[s + 1];
      out[dst + x * 3 + 2] = rgb[s];
    }
  }
  return out;
}

// ---------- rendering (in the page) ----------
const PAGE_HELPERS = `
window.loadImg = async (src) => { const i = new Image(); i.src = src; await i.decode(); return i; };
window.raster = async (src, size) => {
  const img = await loadImg(src);
  const c = document.createElement('canvas'); c.width = c.height = size;
  const x = c.getContext('2d'); x.imageSmoothingQuality = 'high'; x.drawImage(img, 0, 0, size, size);
  return { rgba: Array.from(x.getImageData(0, 0, size, size).data), png: c.toDataURL('image/png') };
};
`;

async function main() {
  let chromium;
  try { ({ chromium } = require('playwright')); } catch { ({ chromium } = require('@playwright/test')); }
  const browser = await chromium.launch(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {});
  const page = await browser.newPage();
  await page.setContent('<!doctype html><body></body>');
  await page.evaluate(PAGE_HELPERS);

  const iconLarge = uri('icon-2.svg');
  const iconSmall = uri('icon-2-small.svg');
  const wmAurora = uri('wordmark-b.svg');
  const wmDark = uri('wordmark-b-dark.svg');

  const raster = async (src, size) => {
    const r = await page.evaluate(([s, n]) => raster(s, n), [src, size]);
    return { rgba: Buffer.from(r.rgba), png: Buffer.from(r.png.split(',')[1], 'base64') };
  };
  const save = (file, data) => { fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, data); console.log('wrote', path.relative(ROOT, file), data.length, 'bytes'); };
  const preview = (name, png) => { if (previewDir) { fs.mkdirSync(previewDir, { recursive: true }); fs.writeFileSync(path.join(previewDir, name), png); } };

  // --- icon.ico: sizes up to 48 from the small drawing, 64 and up from the large one; 256 is PNG-compressed, the rest DIB.
  const entries = [];
  for (const size of [...SMALL_SIZES, ...LARGE_SIZES]) {
    const r = await raster(size <= 48 ? iconSmall : iconLarge, size);
    entries.push({ size, data: size >= 256 ? r.png : dibEntry(r.rgba, size) });
    preview(`ico-${size}.png`, r.png);
  }
  const ico = buildIco(entries);
  save(path.join(ICONS, 'icon.ico'), ico);
  save(CENTER_ICON, ico);

  // --- icon.png (512)
  const big = await raster(iconLarge, 512);
  save(path.join(ICONS, 'icon.png'), big.png);

  // --- installer images: drawn at 4x on a canvas, then scaled down once (smooth edges), flattened onto an opaque background.
  const drawBmp = async (w, h, kind) => {
    const r = await page.evaluate(async ({ w, h, kind, iconLarge, wmAurora, wmDark }) => {
      const S = 4;
      const c = document.createElement('canvas'); c.width = w * S; c.height = h * S;
      const x = c.getContext('2d'); x.imageSmoothingQuality = 'high';
      if (kind === 'header') {
        x.fillStyle = '#FFFFFF'; x.fillRect(0, 0, c.width, c.height);
        const wm = await loadImg(wmAurora);
        const hh = 50 * S, ww = hh * (wm.naturalWidth / wm.naturalHeight);
        x.drawImage(wm, (c.width - ww) / 2, (c.height - hh) / 2, ww, hh);
      } else {
        const g = x.createLinearGradient(0, 0, 0, c.height);
        g.addColorStop(0, '#12142E'); g.addColorStop(0.5, '#080913'); g.addColorStop(1, '#030308');
        x.fillStyle = g; x.fillRect(0, 0, c.width, c.height);
        // aurora pool resting on the bottom edge, the island's own touch
        const pool = x.createRadialGradient(c.width / 2, c.height + 10 * S, 0, c.width / 2, c.height + 10 * S, 150 * S);
        pool.addColorStop(0, 'rgba(191,90,242,0.55)'); pool.addColorStop(0.55, 'rgba(94,92,230,0.20)'); pool.addColorStop(1, 'rgba(94,92,230,0)');
        x.fillStyle = pool; x.fillRect(0, 0, c.width, c.height);
        const halo = x.createRadialGradient(c.width / 2, 108 * S, 0, c.width / 2, 108 * S, 96 * S);
        halo.addColorStop(0, 'rgba(197,140,255,0.22)'); halo.addColorStop(1, 'rgba(197,140,255,0)');
        x.fillStyle = halo; x.fillRect(0, 0, c.width, c.height);
        const icon = await loadImg(iconLarge);
        const is = 100 * S;
        x.drawImage(icon, (c.width - is) / 2, 58 * S, is, is);
        const wm = await loadImg(wmDark);
        const wh = 56 * S, ww = wh * (wm.naturalWidth / wm.naturalHeight);
        x.drawImage(wm, (c.width - ww) / 2, 182 * S, ww, wh);
      }
      const o = document.createElement('canvas'); o.width = w; o.height = h;
      const ox = o.getContext('2d'); ox.imageSmoothingQuality = 'high'; ox.drawImage(c, 0, 0, w, h);
      return { rgba: Array.from(ox.getImageData(0, 0, w, h).data), png: o.toDataURL('image/png') };
    }, { w, h, kind, iconLarge, wmAurora, wmDark });
    return { rgba: Buffer.from(r.rgba), png: Buffer.from(r.png.split(',')[1], 'base64') };
  };
  const header = await drawBmp(150, 57, 'header');
  save(path.join(NSIS, 'installer-header.bmp'), bmp24(header.rgba, 150, 57));
  preview('installer-header.png', header.png);
  const sidebar = await drawBmp(164, 314, 'sidebar');
  save(path.join(NSIS, 'installer-sidebar.bmp'), bmp24(sidebar.rgba, 164, 314));
  preview('installer-sidebar.png', sidebar.png);

  await Promise.race([browser.close(), new Promise((r) => setTimeout(r, 3000))]);
  process.exit(0);
}

main().catch((e) => { console.error(e); process.exit(1); });
