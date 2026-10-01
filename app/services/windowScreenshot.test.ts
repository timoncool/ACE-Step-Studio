import { afterEach, describe, expect, it, vi } from 'vitest';
import { domToPng, type Options } from 'modern-screenshot';
import { captureWindow } from './windowScreenshot';

vi.mock('modern-screenshot', () => ({ domToPng: vi.fn() }));
afterEach(() => { vi.unstubAllGlobals(); vi.resetAllMocks(); });

function scene() {
  const images: Array<{ remove: ReturnType<typeof vi.fn> }> = [];
  const makeDocument = (width: number, height: number) => {
    const doc = {
      documentElement: {},
      defaultView: { innerWidth: width, innerHeight: height, getComputedStyle: () => ({ visibility: 'visible', display: 'block', backgroundColor: '#111' }) },
      body: { appendChild: vi.fn() },
      querySelectorAll: vi.fn((_selector: string) => [] as unknown[]),
      createElement: () => {
        const img = { src: '', style: {}, decode: async () => {}, remove: vi.fn() };
        images.push(img);
        return img;
      },
    };
    return doc;
  };
  const parent = makeDocument(1440, 960);
  const child = makeDocument(1200, 800);
  const frame = { ownerDocument: parent, contentDocument: child, getBoundingClientRect: () => ({ left: 100, top: 80, width: 1200, height: 800 }) };
  parent.querySelectorAll.mockImplementation(selector => selector === 'iframe' ? [frame] : []);
  vi.stubGlobal('document', parent);
  vi.stubGlobal('window', parent.defaultView);
  return { parent, child, images };
}

describe('embedded frame screenshots', () => {
  it('captures the child in its own viewport and excludes parent-context recursion', async () => {
    const { parent, child, images } = scene();
    vi.mocked(domToPng).mockResolvedValue('data:image/png;base64,AA==');
    await captureWindow(720);
    expect(domToPng).toHaveBeenNthCalledWith(1, child.documentElement, expect.objectContaining({ width: 1200, height: 800, scale: 0.5 }));
    expect(domToPng).toHaveBeenNthCalledWith(2, parent.documentElement, expect.objectContaining({ width: 1440, height: 960, scale: 0.5 }));
    const calls = vi.mocked(domToPng).mock.calls as unknown as Array<[Node, Options]>;
    const options = calls[1][1];
    expect(options.filter!({ nodeName: 'IFRAME' } as Node)).toBe(false);
    expect(images[0].remove).toHaveBeenCalledOnce();
  });

  it('cleans up the temporary frame image when parent capture fails', async () => {
    const { images } = scene();
    vi.mocked(domToPng).mockResolvedValueOnce('data:image/png;base64,AA==').mockRejectedValueOnce(new Error('image decode failed'));
    await expect(captureWindow(1600)).rejects.toThrow('image decode failed');
    expect(images[0].remove).toHaveBeenCalledOnce();
  });
});
