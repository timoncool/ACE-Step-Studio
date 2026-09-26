/**
 * A Winamp skin drawn sharp at any scale. Webamp paints every sprite pixel for pixel, so
 * at a scale that is not whole (120 %, 150 %, a 125 % Windows display) its pixels come out
 * of uneven sizes. Here each sprite is resampled once, per skin and scale, to the screen's
 * own pixels by area: a screen pixel takes the skin pixels it covers, in the share it covers
 * them, so every skin pixel stays the same size and only the screen pixels a skin pixel's
 * edge cuts through are mixed. Webamp's skin rules then get the result through image-set()
 * at that resolution, so the browser lays it one to one on the screen instead of stretching
 * the sprite itself. At a whole scale it is the plain pixel-for-pixel enlargement.
 */

/** Sprites Webamp reads back pixel by pixel into its equalizer graph: they stay as they are. */
const READ_AS_PIXELS = new Set(['EQ_PREAMP_LINE', 'EQ_GRAPH_LINE_COLORS']);

function loadImage(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error('a skin sprite did not load'));
    image.src = url;
  });
}

/** For each screen pixel along one side: the skin pixels it covers and how much of it each covers. */
function coverage(source: number, target: number, scale: number): { from: number; weights: number[] }[] {
  return Array.from({ length: target }, (_, pixel) => {
    const start = pixel / scale;
    const end = Math.min(source, (pixel + 1) / scale);
    const from = Math.floor(start);
    const weights: number[] = [];
    for (let at = from; at < end; at += 1) weights.push((Math.min(end, at + 1) - Math.max(start, at)) * scale);
    return { from, weights };
  });
}

/** One pass of the resampling along one axis, on premultiplied colour so transparent pixels add nothing. */
function pass(data: Float32Array, width: number, height: number, cover: { from: number; weights: number[] }[], alongX: boolean): Float32Array {
  const outWidth = alongX ? cover.length : width;
  const outHeight = alongX ? height : cover.length;
  const out = new Float32Array(outWidth * outHeight * 4);
  for (let y = 0; y < outHeight; y += 1) {
    for (let x = 0; x < outWidth; x += 1) {
      const { from, weights } = cover[alongX ? x : y];
      let r = 0;
      let g = 0;
      let b = 0;
      let a = 0;
      let total = 0;
      weights.forEach((weight, step) => {
        const index = alongX ? (y * width + from + step) * 4 : ((from + step) * width + x) * 4;
        r += data[index] * weight;
        g += data[index + 1] * weight;
        b += data[index + 2] * weight;
        a += data[index + 3] * weight;
        total += weight;
      });
      const target = (y * outWidth + x) * 4;
      out[target] = r / total;
      out[target + 1] = g / total;
      out[target + 2] = b / total;
      out[target + 3] = a / total;
    }
  }
  return out;
}

async function resample(url: string, scale: number): Promise<string> {
  const image = await loadImage(url);
  const { naturalWidth: width, naturalHeight: height } = image;
  if (!width || !height) return url;
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext('2d', { willReadFrequently: true });
  if (!context) throw new Error('no 2D canvas for the skin');
  context.drawImage(image, 0, 0);
  const pixels = context.getImageData(0, 0, width, height).data;
  const premultiplied = new Float32Array(pixels.length);
  for (let index = 0; index < pixels.length; index += 4) {
    const alpha = pixels[index + 3] / 255;
    premultiplied[index] = pixels[index] * alpha;
    premultiplied[index + 1] = pixels[index + 1] * alpha;
    premultiplied[index + 2] = pixels[index + 2] * alpha;
    premultiplied[index + 3] = pixels[index + 3];
  }
  const targetWidth = Math.ceil(width * scale);
  const targetHeight = Math.ceil(height * scale);
  const across = pass(premultiplied, width, height, coverage(width, targetWidth, scale), true);
  const done = pass(across, targetWidth, height, coverage(height, targetHeight, scale), false);
  const result = new ImageData(targetWidth, targetHeight);
  for (let index = 0; index < done.length; index += 4) {
    const alpha = done[index + 3];
    const unmultiply = alpha > 0 ? 255 / alpha : 0;
    result.data[index] = Math.round(done[index] * unmultiply);
    result.data[index + 1] = Math.round(done[index + 1] * unmultiply);
    result.data[index + 2] = Math.round(done[index + 2] * unmultiply);
    result.data[index + 3] = Math.round(alpha);
  }
  canvas.width = targetWidth;
  canvas.height = targetHeight;
  context.putImageData(result, 0, 0);
  return canvas.toDataURL();
}

/** Screen pixels per skin pixel, to the hundredth: the scale a skin is resampled for. */
export function skinScale(screenPixels: number): number {
  return Math.round(screenPixels * 100) / 100;
}

/** Skins already resampled, by skin and scale: going back to a skin or a scale takes them from here. */
const done = new Map<string, Promise<Map<string, string>>>();
const KEPT = 12;

/** Each sprite of the skin, by its address in Webamp's rules, resampled to `scale` screen pixels per skin pixel. */
export function sharpenSkin(images: Record<string, string>, scale: number): Promise<Map<string, string>> {
  if (scale === 1) return Promise.resolve(new Map());
  const key = `${scale} ${images.MAIN_WINDOW_BACKGROUND ?? Object.values(images).join('')}`;
  const kept = done.get(key);
  if (kept) {
    done.delete(key);
    done.set(key, kept);
    return kept;
  }
  const work = Promise.all(
    Object.entries(images)
      .filter(([name]) => !READ_AS_PIXELS.has(name))
      .map(async ([, url]) => [url, await resample(url, scale)] as const),
  ).then((entries) => new Map(entries));
  done.set(key, work);
  work.catch(() => done.delete(key));
  if (done.size > KEPT) done.delete(done.keys().next().value as string);
  return work;
}

const SKIN_RULE = /([^{}]+)\{background-image: url\(([^)]+)\)\}/g;

/**
 * Webamp's skin rules with the resampled sprites put in, at their resolution. The picture is
 * a hair wider than the sprite where the scale does not divide it evenly; the box cuts that.
 */
export function sharpSkinCss(skinCss: string, sharp: Map<string, string>, scale: number): string {
  const rules: string[] = [];
  for (const [, selector, url] of skinCss.matchAll(SKIN_RULE)) {
    const resampled = sharp.get(url);
    if (resampled) rules.push(`${selector.trim()} {background-image: image-set(url("${resampled}") ${scale}x) !important}`);
  }
  return rules.join('\n');
}
