import type { Butterchurn } from 'butterchurn';

/// butterchurn is CommonJS with the class under `exports.default`; the default import is the whole `module.exports`.
export async function loadButterchurn(): Promise<Butterchurn> {
  return (await import('butterchurn')).default.default;
}
