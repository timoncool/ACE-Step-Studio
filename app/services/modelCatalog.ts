import { translations, type TranslationKey } from '../i18n/translations';
export type ModelComponent = {
  id: string;
  kind: string;
  filename: string;
  bytes: number;
  sha256: string;
};

/** The roles of a runnable set, in the order the model manager lists them. */
export const COMPONENT_KINDS = ['dit', 'lm', 'text_encoder', 'vae'] as const;

const labels: Record<(typeof COMPONENT_KINDS)[number], string> = {
  dit: 'DiT',
  lm: 'Language model',
  text_encoder: 'Text encoder',
  vae: 'VAE',
};

export const componentKindLabel = (kind: string) => labels[kind as keyof typeof labels] || kind;

export const componentPrecision = (component: ModelComponent) => {
  const matched = component.filename.match(/-(BF16|F32|MXFP4|Q\d+(?:_K(?:_M)?|_0)?)\.gguf$/i);
  return matched?.[1]?.toUpperCase() || component.id;
};

/** The model a DiT file is, without its quantisation: xl-turbo, merge-sft-turbo-xl-ta-0.3... */
export const componentVariant = (component: ModelComponent) =>
  component.filename
    .replace(/^acestep-v15-/, '')
    .replace(/^acestep-5Hz-lm-/, 'LM ')
    .replace(/-(BF16|F32|MXFP4|Q\d+(?:_K(?:_M)?|_0)?)\.gguf$/i, '');

export const componentsByKind = (components: ModelComponent[]) =>
  COMPONENT_KINDS.map((kind) => ({ kind, components: components.filter((component) => component.kind === kind) }));

export const completeCustomComponentIds = (components: ModelComponent[], selectedByKind: Record<string, string>) => {
  const ids = COMPONENT_KINDS.map((kind) => selectedByKind[kind]).filter((id): id is string => Boolean(id));
  if (ids.length !== COMPONENT_KINDS.length || new Set(ids).size !== ids.length) return null;
  const selected = ids.map((id) => components.find((component) => component.id === id));
  if (selected.some((component) => !component)) return null;
  if (new Set(selected.map((component) => component!.kind)).size !== COMPONENT_KINDS.length) return null;
  return ids;
};

export const selectedComponentBytes = (components: ModelComponent[], ids: string[] | null) =>
  ids?.reduce((total, id) => total + (components.find((component) => component.id === id)?.bytes || 0), 0) || 0;

/** A set's name in the reader's language, where the interface has one. */
export function profileLabel(t: (key: TranslationKey) => string, profile: { id: string; label: string }): string {
  const key = `profile_${profile.id}`;
  return key in translations.en ? t(key as TranslationKey) : profile.label;
}
