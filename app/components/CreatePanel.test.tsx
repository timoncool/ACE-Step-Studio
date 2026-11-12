import React, { Activity, act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AceCreateRequest, Song } from '../types';

const setup = vi.hoisted(() => ({ synthModel: undefined as string | undefined }));

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => key }) }));
vi.mock('../services/studioQueries', () => ({
  SELF_READ: {},
  libraryChanged: vi.fn(),
  setupStatusChanged: vi.fn(),
  updateLibrarySongs: vi.fn(),
  useActivity: () => ({ data: [] }),
  useAssistantStatus: () => ({ data: null }),
  useLibrarySongs: () => ({ data: [] }),
  useSetupStatus: () => ({ isError: false, data: { ready: true, engine_ready: true, profile_files: { synth_model: setup.synthModel } } }),
}));
vi.mock('../services/mcpBridge', () => ({ useBridgeCommand: () => undefined }));
vi.mock('./AdapterPicker', () => ({ AdapterPicker: () => null }));
vi.mock('./ModelSwitcher', () => ({ ModelSwitcher: () => null }));
vi.mock('./PlaylistModals', () => ({ CreatePlaylistModal: () => null }));
vi.mock('./SlideToEnable', () => ({ SlideToEnable: () => null }));

const { CreatePanel } = await import('./CreatePanel');

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;

beforeEach(() => {
  localStorage.clear();
  setup.synthModel = undefined;
  vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({}), { status: 200 })));
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

const flush = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });

function panel(props: { initialData?: { song: Song; timestamp: number } | null; visible?: boolean; onGenerate?: (request: AceCreateRequest) => void }) {
  return (
    <Activity mode={props.visible === false ? 'hidden' : 'visible'}>
      <CreatePanel onGenerate={props.onGenerate ?? (() => undefined)} isGenerating={false} initialData={props.initialData ?? null}
        playlists={[]} onCreatePlaylist={async () => null} playlistId="" onChoosePlaylist={() => undefined} />
    </Activity>
  );
}

const type = (element: HTMLTextAreaElement, value: string) => act(() => {
  const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(element), 'value')!.set!;
  setter.call(element, value);
  element.dispatchEvent(new Event('input', { bubbles: true }));
});

describe('Reuse prompt (YuE2 Studio #216)', () => {
  const song = { id: 's', title: 'Song', style: 'pop', lyrics: '[verse]\nfirst words', generationParams: {} } as unknown as Song;
  const lyrics = () => [...host.querySelectorAll('textarea')].find(area => area.value.includes('words'));

  it('keeps lyrics edited after a Reuse when Create is hidden and shown again; a new Reuse loads the song', async () => {
    act(() => root.render(panel({ initialData: { song, timestamp: 1 } })));
    await flush();
    expect(lyrics()?.value).toContain('first words');
    type(lyrics()!, '[verse]\nedited words');
    act(() => root.render(panel({ initialData: { song, timestamp: 1 }, visible: false })));
    await flush();
    act(() => root.render(panel({ initialData: { song, timestamp: 1 } })));
    await flush();
    expect(lyrics()?.value).toContain('edited words');
    act(() => root.render(panel({ initialData: { song, timestamp: 2 } })));
    await flush();
    expect(lyrics()?.value).toContain('first words');
  });
});

describe('Reuse, then another DiT', () => {
  const songWith = (generationParams: Record<string, unknown>) =>
    ({ id: 't', title: 'Turbo song', style: 'house', lyrics: '[Instrumental]', generationParams }) as unknown as Song;
  const sent: AceCreateRequest[] = [];
  const onGenerate = (request: AceCreateRequest) => { sent.push(request); };
  const show = async (song: Song, synthModel: string) => {
    setup.synthModel = synthModel;
    act(() => root.render(panel({ initialData: { song, timestamp: 1 }, onGenerate })));
    await flush();
  };
  const sampling = () => {
    const create = [...host.querySelectorAll('button')].find(button => button.textContent?.includes('create'));
    act(() => create!.click());
    const request = sent.at(-1)!;
    return { steps: request.inference_steps, shift: request.shift, solver: request.solver, scheduler: request.scheduler };
  };

  it('sends another kind of DiT its own preset after a Reuse', async () => {
    const song = songWith({ inference_steps: 12, shift: 2.5, solver: 'heun', scheduler: 'linear_quadratic', guidance_scale: 1 });
    await show(song, 'acestep-v15-turbo-Q8_0.gguf');
    expect(sampling()).toEqual({ steps: 12, shift: 2.5, solver: 'heun', scheduler: 'linear_quadratic' });
    await show(song, 'acestep-v15-base-Q8_0.gguf');
    expect(sampling()).toEqual({ steps: 50, shift: 1, solver: 'euler', scheduler: 'linear' });
  });

  it('keeps the song sampling when the DiT changes within its kind', async () => {
    const song = songWith({ inference_steps: 12, shift: 2.5, solver: 'heun', scheduler: 'linear_quadratic', guidance_scale: 1 });
    await show(song, 'acestep-v15-turbo-Q8_0.gguf');
    expect(sampling()).toEqual({ steps: 12, shift: 2.5, solver: 'heun', scheduler: 'linear_quadratic' });
    await show(song, 'acestep-v15-turbo-Q4_K_M.gguf');
    expect(sampling()).toEqual({ steps: 12, shift: 2.5, solver: 'heun', scheduler: 'linear_quadratic' });
  });
});
