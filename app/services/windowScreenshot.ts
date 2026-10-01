import { domToPng } from 'modern-screenshot';

function isVisible(element: Element): boolean {
  const box = element.getBoundingClientRect();
  const style = element.ownerDocument.defaultView!.getComputedStyle(element);
  return box.width > 0 && box.height > 0 && style.visibility !== 'hidden' && style.display !== 'none';
}

async function freezeCanvases(doc: Document, selector: string): Promise<HTMLImageElement[]> {
  const canvases = [...doc.querySelectorAll<HTMLCanvasElement>(selector)].filter(isVisible);
  if (!canvases.length) return [];
  const win = doc.defaultView!;
  const replacements = new Map<HTMLImageElement, HTMLCanvasElement>();
  return new Promise((resolve, reject) => {
    const timer = win.setTimeout(() => {
      win.cancelAnimationFrame(frame);
      reject(new Error('The canvas did not draw a frame for the screenshot.'));
    }, 3000);
    const frame = win.requestAnimationFrame(() => {
      win.clearTimeout(timer);
      const images: HTMLImageElement[] = [];
      try {
        for (const canvas of canvases) {
          const image = doc.createElement('img');
          for (const attribute of canvas.attributes) image.setAttribute(attribute.name, attribute.value);
          image.src = canvas.toDataURL('image/png');
          // Keep the canvas's layout, clipping and stacking context intact.
          canvas.replaceWith(image);
          replacements.set(image, canvas);
          images.push(image);
        }
        for (const image of images) {
          const canvas = replacements.get(image)!;
          image.remove = () => image.replaceWith(canvas);
        }
        resolve(images);
      } catch (error) {
        images.forEach(image => image.replaceWith(replacements.get(image)!));
        reject(error);
      }
    });
  });
}

async function capture(doc: Document, scale: number): Promise<string> {
  const win = doc.defaultView!;
  const overlays: HTMLImageElement[] = [];
  try {
    for (const frame of doc.querySelectorAll<HTMLIFrameElement>('iframe')) {
      if (!isVisible(frame)) continue;
      const child = frame.contentDocument;
      if (!child?.documentElement) throw new Error('The embedded frame is not accessible for a screenshot.');
      const box = frame.getBoundingClientRect();
      const image = doc.createElement('img');
      image.src = await capture(child, scale);
      Object.assign(image.style, { position: 'fixed', left: `${box.left}px`, top: `${box.top}px`, width: `${box.width}px`, height: `${box.height}px`, zIndex: '2147483646', pointerEvents: 'none' });
      await image.decode();
      doc.body.appendChild(image);
      overlays.push(image);
    }
    // A frame owns its WebGL render loop; read its canvas in that window's draw cycle.
    if (win === window) overlays.push(...await freezeCanvases(doc, 'canvas[data-webgl], #webamp .gen-window canvas'));
    return await domToPng(doc.documentElement, {
      scale, width: win.innerWidth, height: win.innerHeight, timeout: 5000,
      backgroundColor: win.getComputedStyle(doc.body).backgroundColor,
      // Frames were captured using their own resource URLs, styles and viewport.
      filter: node => node.nodeName !== 'IFRAME',
    });
  } finally {
    overlays.forEach(image => image.remove());
  }
}

export async function captureWindow(maxWidth: number): Promise<string> {
  return capture(document, Math.min(1, maxWidth / window.innerWidth));
}
