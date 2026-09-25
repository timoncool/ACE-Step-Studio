export type OpenRouterCapability = 'speech_to_text' | 'cover_art';

export interface NativeOpenRouterModel {
  id: string;
  name: string;
  description?: string | null;
  capabilities: string[];
}

interface CatalogResponse {
  models?: NativeOpenRouterModel[] | null;
}

interface NativeOpenRouterResponse {
  body: unknown;
  generation_id?: string;
}

async function responseError(response: Response): Promise<Error> {
  const fallback = `Native OpenRouter service is unavailable (${response.status}).`;
  try {
    const payload = await response.json() as { error?: unknown };
    return new Error(typeof payload.error === 'string' ? payload.error : fallback);
  } catch {
    return new Error(fallback);
  }
}

/// Every native OpenRouter route is a POST. Sending a bodyless request as a
/// GET made the catalog refresh fail with 405 and surfaced in the UI as
/// "cloud media is unavailable".
async function jsonRequest<T>(path: string, body?: unknown): Promise<T> {
  const response = await fetch(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body ?? {}),
  });
  if (!response.ok) throw await responseError(response);
  return response.json() as Promise<T>;
}

/// Reads the catalog the service already holds, fetching it there if this is
/// the first ask. Every panel calls this on mount so nobody has to press
/// "refresh" to see the models that exist.
export async function loadNativeOpenRouterCatalog(): Promise<NativeOpenRouterModel[]> {
  const response = await fetch('/v1/openrouter/catalog');
  if (!response.ok) throw await responseError(response);
  const body = await response.json() as CatalogResponse;
  return body.models ?? [];
}

export async function refreshNativeOpenRouterCatalog(): Promise<NativeOpenRouterModel[]> {
  const response = await jsonRequest<CatalogResponse>('/v1/openrouter/catalog/refresh');
  return response.models ?? [];
}

export function modelsForCapability(models: NativeOpenRouterModel[], capability: OpenRouterCapability): NativeOpenRouterModel[] {
  return models.filter((model) => model.capabilities.includes(capability));
}

async function base64For(file: File): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = '';
  const chunkSize = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize));
  }
  return btoa(binary);
}

export async function transcribeWithNativeOpenRouter(modelId: string, file: File, language?: string): Promise<string> {
  const response = await jsonRequest<NativeOpenRouterResponse>('/v1/openrouter/transcriptions', {
    model_id: modelId,
    audio_base64: await base64For(file),
    audio_format: audioFormatFor(file),
    language: language?.trim() || undefined,
  });
  const text = (response.body as { text?: unknown })?.text;
  if (typeof text !== 'string') throw new Error('OpenRouter returned no transcription text.');
  return text;
}

export async function generateCoverWithNativeOpenRouter(modelId: string, prompt: string): Promise<string> {
  const response = await jsonRequest<NativeOpenRouterResponse>('/v1/openrouter/covers', {
    model_id: modelId,
    prompt,
  });
  const image = (response.body as { data?: Array<{ b64_json?: unknown; media_type?: unknown }> })?.data?.[0];
  if (!image || typeof image.b64_json !== 'string') throw new Error('OpenRouter returned no image data.');
  const mediaType = typeof image.media_type === 'string' ? image.media_type : 'image/png';
  return `data:${mediaType};base64,${image.b64_json}`;
}

function audioFormatFor(file: File): string {
  const byMime: Record<string, string> = {
    'audio/wav': 'wav', 'audio/mpeg': 'mp3', 'audio/flac': 'flac', 'audio/mp4': 'm4a',
    'audio/ogg': 'ogg', 'audio/webm': 'webm', 'audio/aac': 'aac',
  };
  const byExtension = file.name.split('.').pop()?.toLowerCase();
  return byMime[file.type] || byExtension || 'wav';
}
