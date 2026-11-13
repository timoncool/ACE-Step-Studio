/** Who this studio is, from the `studio.json` it was built with. */
export const STUDIO = __STUDIO__;

export const STUDIO_REPO_URL = `https://github.com/${STUDIO.repo}`;

/** The studio's site, its donation page among its pages: the repository's GitHub Pages. */
export const STUDIO_SITE_URL = `https://${STUDIO.repo.split('/')[0]}.github.io/${STUDIO.repo.split('/')[1]}/`;
