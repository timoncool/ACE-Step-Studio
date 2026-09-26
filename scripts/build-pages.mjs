// Renders the project page into docs/index.html (English) and docs/<lang>.html, and
// docs/sitemap.xml. Every page is plain HTML in its own language, with the app and its
// questions described for search engines and AI answers (schema.org JSON-LD), so readers
// without JavaScript, crawlers and assistants all see the same page.
import { openSync, readFileSync, readSync, closeSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ORDER, SAMPLES, SETS, STRINGS } from '../docs/site/strings.mjs';
import { STUDIO } from '../docs/site/studio.mjs';

const docs = join(dirname(fileURLToPath(import.meta.url)), '..', 'docs');
const css = readFileSync(join(docs, 'site', 'page.css'), 'utf8');
const { name: NAME, repo: REPO, site: SITE } = STUDIO;

// The studio family as tabs over the page: the same language on each site.
const FAMILY = [
  ['yue2', 'YuE2 Studio', 'https://timoncool.github.io/YuE2-Studio/'],
  ['minimax', 'MiniMax Music3 Studio', 'https://timoncool.github.io/MiniMax-Music3-Studio/'],
  ['ace', 'ACE-Step Studio', 'https://timoncool.github.io/ACE-Step-Studio/'],
];

// The questions every studio answers the same way; the answers come from the studio's own strings.
const WORDS = {
  en: { starCta: 'Star on GitHub', download: 'Download', moreSamples: 'More examples', faqTitle: 'Questions', qGpu: 'What graphics card do I need?', qFree: 'Is it free?', aFree: 'Yes. The studio is free and open source, and the models download for free from inside it.', qOffline: 'Does it work offline? Is my music sent anywhere?', qLicense: 'Can I use the songs I make?', qLora: 'Can I train it on my own style?', qAgent: 'Can an AI agent use it?', qInside: 'What is inside the download?', qBuilt: 'How is it built?', updated: 'Updated', stars: 'GitHub stars' },
  ru: { starCta: 'Поставить звезду', download: 'Скачать', moreSamples: 'Ещё примеры', faqTitle: 'Вопросы', qGpu: 'Какая нужна видеокарта?', qFree: 'Это бесплатно?', aFree: 'Да. Студия бесплатная и с открытым кодом, модели скачиваются бесплатно прямо из неё.', qOffline: 'Работает ли без интернета? Уходит ли куда-то моя музыка?', qLicense: 'Можно ли использовать сделанные песни?', qLora: 'Можно ли обучить её своему стилю?', qAgent: 'Может ли ею управлять ИИ-агент?', qInside: 'Что внутри установщика?', qBuilt: 'Как она устроена?', updated: 'Обновлено', stars: 'Звёзды на GitHub' },
  zh: { starCta: '在 GitHub 上加星', download: '下载', moreSamples: '更多示例', faqTitle: '常见问题', qGpu: '需要什么显卡？', qFree: '免费吗？', aFree: '是的。工作室免费且开源，模型也可以在其中免费下载。', qOffline: '没有网络能用吗？我的音乐会被上传吗？', qLicense: '生成的歌曲可以使用吗？', qLora: '能用我自己的风格训练吗？', qAgent: 'AI 智能体能操作它吗？', qInside: '安装包里有什么？', qBuilt: '它是如何构建的？', updated: '更新于', stars: 'GitHub 星标' },
  ja: { starCta: 'GitHub でスター', download: 'ダウンロード', moreSamples: 'その他のサンプル', faqTitle: 'よくある質問', qGpu: 'どんなグラフィックカードが必要ですか？', qFree: '無料ですか？', aFree: 'はい。スタジオは無料のオープンソースで、モデルもスタジオ内から無料でダウンロードできます。', qOffline: 'オフラインで使えますか？ 音楽はどこかに送られますか？', qLicense: '作った曲は使えますか？', qLora: '自分のスタイルで学習させられますか？', qAgent: 'AI エージェントから操作できますか？', qInside: 'ダウンロードの中身は？', qBuilt: 'どのように作られていますか？', updated: '更新日', stars: 'GitHub スター' },
  ko: { starCta: 'GitHub에서 스타 주기', download: '다운로드', moreSamples: '더 많은 예시', faqTitle: '자주 묻는 질문', qGpu: '어떤 그래픽 카드가 필요한가요?', qFree: '무료인가요?', aFree: '네. 스튜디오는 무료 오픈 소스이며, 모델도 스튜디오 안에서 무료로 내려받습니다.', qOffline: '인터넷 없이 되나요? 내 음악이 어딘가로 전송되나요?', qLicense: '만든 곡을 사용할 수 있나요?', qLora: '내 스타일로 학습시킬 수 있나요?', qAgent: 'AI 에이전트가 사용할 수 있나요?', qInside: '다운로드에는 무엇이 들어 있나요?', qBuilt: '어떻게 만들어졌나요?', updated: '업데이트', stars: 'GitHub 스타' },
};

const escape = (text) => String(text)
  .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const file = (lang) => (lang === 'en' ? 'index.html' : `${lang}.html`);
const pageUrl = (lang) => `${SITE}${lang === 'en' ? '' : file(lang)}`;
const shot = (lang, name) => `screenshots/${lang}-${name}.webp`;
/** A screenshot's width and height, from its PNG header. */
function pictureSize(lang, name) {
  const handle = openSync(join(docs, 'screenshots', `${lang}-${name}.png`), 'r');
  const head = Buffer.alloc(24);
  readSync(handle, head, 0, 24, 0);
  closeSync(handle);
  return [head.readUInt32BE(16), head.readUInt32BE(20)];
}
const starsBadge = (alt) => `<img class="stars" src="https://img.shields.io/github/stars/${REPO.replace('https://github.com/', '')}?style=flat&amp;label=%E2%98%85&amp;color=3f3f46" alt="${alt}" height="20" />`;

/** The first sentence of a text: the one line a tile or the hero has room for. */
function firstSentence(text) {
  const cjk = text.match(/^(.+?[。！？])/);
  const latin = text.match(/^(.+?[.!?])(\s|$)/);
  const cut = [cjk?.[1], latin?.[1]].filter(Boolean).sort((a, b) => a.length - b.length)[0];
  return cut ?? text;
}

function page(lang) {
  const s = STRINGS[lang];
  const w = WORDS[lang];
  for (const key of Object.keys(STRINGS.en)) {
    if (!(key in s)) throw new Error(`[ERROR] ${lang} is missing "${key}"`);
  }
  const shotAlt = (name) => {
    const found = s.shots.find(([id]) => id === name);
    if (!found) throw new Error(`[ERROR] no screenshot ${name} in ${lang}`);
    return found[1];
  };

  const alternates = [
    ...ORDER.map((code) => `<link rel="alternate" hreflang="${code}" href="${pageUrl(code)}" />`),
    `<link rel="alternate" hreflang="x-default" href="${pageUrl('en')}" />`,
  ].join('\n');
  const family = FAMILY
    .map(([id, label, url]) => `    <a href="${url}${lang === 'en' ? '' : file(lang)}"${id === STUDIO.id ? ' aria-current="page"' : ''}>${label}</a>`)
    .join('\n');
  const nav = ORDER
    .map((code) => `      <a href="${file(code)}" hreflang="${code}"${code === lang ? ' aria-current="page"' : ''}>${escape(STRINGS[code].label)}</a>`)
    .join('\n');

  const download = `<a class="btn primary" href="${REPO}/releases/latest">${escape(s.ctaDownload)}<small data-release>v${STUDIO.version} · ${STUDIO.installerMB} MB</small></a>`;

  const tiles = STUDIO.bento.map(({ feature, shot: name }) => {
    const [title, body] = s.features[feature];
    const [width, height] = pictureSize(lang, name);
    return `      <article class="tile">
        <h3>${escape(title)}</h3>
        <figure class="shot"><img src="${shot(lang, name)}" alt="${escape(shotAlt(name))}" loading="lazy" width="${width}" height="${height}" /></figure>
        <p>${escape(firstSentence(body))}</p>
      </article>`;
  }).join('\n');

  // the gallery holds the screenshots the hero and the tiles do not already show
  const shown = new Set(['01-create', ...STUDIO.bento.map(({ shot: name }) => name)]);
  const sampleCard = (sample) => `      <figure class="sample"><figcaption><b>${escape(sample.title)}</b><span>${escape(sample.note[lang])}</span></figcaption><audio controls preload="none" src="samples/${sample.file}"></audio></figure>`;
  const samples = SAMPLES.length ? `  <section id="listen">
    <h2>${escape(s.samplesTitle)}</h2>
    <p class="sub">${escape(s.samplesSub)}</p>
    <div class="samples">
${SAMPLES.slice(0, 6).map(sampleCard).join('\n')}
    </div>
${SAMPLES.length > 6 ? `    <details class="more"><summary>${escape(w.moreSamples)} (${SAMPLES.length - 6})</summary><div class="samples">
${SAMPLES.slice(6).map(sampleCard).join('\n')}
    </div></details>
` : ''}  </section>
` : '';

  // a step's text was written to follow its title after a dash; in a card it starts a sentence
  const capital = (text) => text.charAt(0).toLocaleUpperCase(lang) + text.slice(1);
  const steps = s.steps
    .map(([title, body], index) => `      <li><b><span class="num">${index + 1}</span>${escape(title.replace(/\s*[—-]\s*$/, ''))}</b><span>${escape(capital(body))}</span></li>`)
    .join('\n');

  // Questions and answers: shown as they are, and handed to search engines and AI answers as FAQPage.
  const setsTable = `<div class="scroll"><table>
          <thead><tr><th>${escape(s.modelsGpu)}</th><th>${escape(s.modelsSet)}</th><th>${escape(s.modelsSize)}</th></tr></thead>
          <tbody>
${SETS.map((set) => `            <tr><td>${set.vram}</td><td>${escape(s[set.key])}</td><td>${set.size}</td></tr>`).join('\n')}
          </tbody>
        </table></div>`;
  const insideTable = `<div class="scroll"><table>
          <thead><tr><th>${escape(s.insidePart)}</th><th>${escape(s.insideRuns)}</th><th>${escape(s.insideSize)}</th></tr></thead>
          <tbody>
${s.inside.map((row) => `            <tr>${row.map((cell) => `<td>${escape(cell)}</td>`).join('')}</tr>`).join('\n')}
          </tbody>
        </table></div>`;
  const faq = [
    { q: w.qGpu, text: [s.ctaNote, s.modelsSub, s.modelsNote], html: `<p>${escape(s.ctaNote)}</p><p>${escape(s.modelsSub)}</p>${setsTable}<p class="note">${escape(s.modelsNote)}</p>` },
    { q: w.qFree, text: [w.aFree], html: `<p>${escape(w.aFree)}</p>` },
    { q: w.qOffline, text: [s.privacySub, ...s.privacy.map(([t, b]) => `${t}: ${b}`)], html: `<p>${escape(s.privacySub)}</p><ul>${s.privacy.map(([t, b]) => `<li><b>${escape(t)}</b> — ${escape(b)}</li>`).join('')}</ul>` },
    { q: w.qLicense, text: [s.footerLicense], html: `<p>${escape(s.footerLicense)}</p>` },
    { q: w.qLora, text: [s.features[STUDIO.faq.lora][1]], html: `<p>${escape(s.features[STUDIO.faq.lora][1])}</p>` },
    { q: w.qAgent, text: [s.features[STUDIO.faq.mcp][1]], html: `<p>${escape(s.features[STUDIO.faq.mcp][1])}</p>` },
    { q: w.qInside, text: [s.insideSub], html: `<p>${escape(s.insideSub)}</p>${insideTable}` },
    { q: w.qBuilt, text: [s.archSub], html: `<p>${escape(s.archSub)}</p><pre>${escape(STUDIO.arch)}</pre>` },
  ];

  const author = { '@type': 'Person', '@id': 'https://github.com/timoncool#person', name: 'Nerual Dreming', url: 'https://github.com/timoncool',
    sameAs: ['https://github.com/timoncool', 'https://t.me/nerual_dreming', 'https://artgeneration.me'] };
  const ld = {
    '@context': 'https://schema.org',
    '@graph': [
      {
        '@type': 'SoftwareApplication', '@id': `${SITE}#app`, name: NAME, url: pageUrl(lang), inLanguage: lang,
        description: s.description, applicationCategory: 'MultimediaApplication', applicationSubCategory: 'AI music generation',
        operatingSystem: 'Windows 10, Windows 11', softwareVersion: STUDIO.version, fileSize: `${STUDIO.installerMB} MB`,
        downloadUrl: `${REPO}/releases/latest`, isAccessibleForFree: true, license: 'https://opensource.org/licenses/MIT',
        offers: { '@type': 'Offer', price: '0', priceCurrency: 'USD' },
        image: `${SITE}og-${lang}.jpg`, screenshot: `${SITE}${shot(lang, '01-create')}`,
        codeRepository: REPO, sameAs: [REPO], author: { '@id': author['@id'] }, dateModified: STUDIO.updated,
      },
      author,
      { '@type': 'FAQPage', inLanguage: lang, mainEntity: faq.map(({ q, text }) => ({ '@type': 'Question', name: q, acceptedAnswer: { '@type': 'Answer', text: text.join(' ') } })) },
    ],
  };

  return `<!doctype html>
<html lang="${lang}">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>${escape(s.title)}</title>
<meta name="description" content="${escape(s.description)}" />
<link rel="canonical" href="${pageUrl(lang)}" />
${alternates}
<link rel="icon" type="image/png" href="logo.png" />
<meta property="og:type" content="website" />
<meta property="og:site_name" content="${NAME}" />
<meta property="og:title" content="${escape(s.title)}" />
<meta property="og:description" content="${escape(s.description)}" />
<meta property="og:url" content="${pageUrl(lang)}" />
<meta property="og:locale" content="${lang}" />
<meta property="og:image" content="${SITE}og-${lang}.jpg" />
<meta property="og:image:width" content="1200" />
<meta property="og:image:height" content="630" />
<meta property="og:image:alt" content="${escape(s.heroTitle)}" />
<meta name="twitter:card" content="summary_large_image" />
<meta name="twitter:image" content="${SITE}og-${lang}.jpg" />
<link rel="preload" as="image" href="${shot(lang, '01-create')}" fetchpriority="high" />
<script type="application/ld+json">${JSON.stringify(ld).replace(/</g, '\\u003c')}</script>
<style>
${css.trimEnd()}
</style>
</head>
<body>
<header>
  <div class="utility"><div class="wrap utility-bar">
    <nav class="family" aria-label="Studios">
${family}
    </nav>
    <nav class="langs" aria-label="Language">
${nav}
    </nav>
  </div></div>
  <div class="wrap bar">
    <img class="mark" src="logo.png" alt="" width="30" height="30" />
    <div class="name">${NAME}</div>
    <a class="head-github" href="${REPO}">GitHub${starsBadge(escape(w.stars))}</a>
    <a class="head-download" href="${REPO}/releases/latest">${escape(w.download)}</a>
  </div>
</header>

<main class="wrap">
  <section class="hero">
    <h1>${escape(s.heroTitle)}</h1>
    <p class="lead">${escape(firstSentence(s.heroLead))}</p>
    <div class="cta">
      ${download}
      <a class="btn" href="${REPO}">${escape(w.starCta)}${starsBadge(escape(w.stars))}</a>
    </div>
    <p class="note">${escape(s.ctaNote)}</p>
    <figure class="hero-shot"><img src="${shot(lang, '01-create')}" alt="${escape(shotAlt('01-create'))}" width="1600" height="1000" fetchpriority="high" /></figure>
  </section>

${samples}  <section id="features">
    <h2>${escape(s.featuresTitle)}</h2>
    <div class="bento">
${tiles}
    </div>
  </section>

  <section id="screenshots">
    <h2>${escape(s.shotsTitle)}</h2>
    <p class="sub">${escape(s.shotsSub)}</p>
    <div class="gallery">
${s.shots.filter(([name]) => !shown.has(name)).map(([name, caption]) => `      <figure><img src="${shot(lang, name)}" alt="${escape(caption)}" loading="lazy" width="1600" height="1000" /><figcaption>${escape(caption)}</figcaption></figure>`).join('\n')}
    </div>
  </section>

  <section id="start">
    <h2>${escape(s.startTitle)}</h2>
    <ol class="steps">
${steps}
    </ol>
    <div class="cta">${download}</div>
  </section>

  <section id="faq">
    <h2>${escape(w.faqTitle)}</h2>
${faq.map(({ q, html }) => `    <details><summary>${escape(q)}</summary><div class="answer">${html}</div></details>`).join('\n')}
  </section>

  <section id="author">
    <h2>${escape(s.authorTitle)}</h2>
    <p class="sub">${escape(s.authorSub)}</p>
    <div class="pills">
      <a class="pill" href="https://github.com/timoncool">GitHub · @timoncool</a>
      <a class="pill" href="https://t.me/nerual_dreming">Telegram · @nerual_dreming</a>
      <a class="pill" href="https://t.me/neuroport">Telegram · @neuroport</a>
      <a class="pill" href="https://artgeneration.me">ArtGeneration.me</a>
      <a class="pill" href="${REPO}/blob/main/DONATE.md">${escape(s.ctaDonate)}</a>
    </div>
  </section>

  <footer>
    <p>${escape(s.footerLicense)}</p>
    <p><a href="${REPO}/blob/main/CHANGELOG.md">${escape(s.footerChanges)}</a> · <a href="${REPO}/issues">${escape(s.footerIssues)}</a> · ${escape(w.updated)} <time datetime="${STUDIO.updated}">${STUDIO.updated}</time></p>
  </footer>
</main>

<div class="viewer" id="viewer" aria-hidden="true">
  <button type="button" aria-label="Close">×</button>
  <img id="viewer-image" alt="" />
</div>
<script>
const viewer = document.getElementById('viewer');
const viewerImage = document.getElementById('viewer-image');
const closeViewer = () => { viewer.classList.remove('open'); viewer.setAttribute('aria-hidden', 'true'); };
document.addEventListener('click', (event) => {
  const image = event.target.closest('.hero-shot img, .tile img, .gallery img');
  if (image) {
    viewerImage.src = image.src;
    viewerImage.alt = image.alt;
    viewer.classList.add('open');
    viewer.setAttribute('aria-hidden', 'false');
    return;
  }
  if (event.target.closest('#viewer')) closeViewer();
});
document.addEventListener('keydown', (event) => { if (event.key === 'Escape') closeViewer(); });
// the download buttons name the latest release as GitHub has it, so a new release needs no new page
fetch('https://api.github.com/repos/${REPO.replace('https://github.com/', '')}/releases/latest')
  .then((response) => (response.ok ? response.json() : Promise.reject(new Error('GitHub answered ' + response.status))))
  .then((release) => {
    const installer = release.assets.find((asset) => asset.name.endsWith('-setup.exe'));
    if (!installer) throw new Error('the latest release has no installer');
    const label = release.tag_name + ' · ' + Math.round(installer.size / 1048576) + ' MB';
    document.querySelectorAll('[data-release]').forEach((element) => { element.textContent = label; });
  })
  .catch((error) => console.warn('Latest release not read; the page shows the one it was built with:', error));
</script>
</body>
</html>
`;
}

for (const lang of ORDER) {
  writeFileSync(join(docs, file(lang)), page(lang), 'utf8');
  console.log(`[OK] docs/${file(lang)}`);
}

// The sitemap names every language of the page and ties them together, as the pages do.
const links = [...ORDER.map((code) => `    <xhtml:link rel="alternate" hreflang="${code}" href="${pageUrl(code)}"/>`),
  `    <xhtml:link rel="alternate" hreflang="x-default" href="${pageUrl('en')}"/>`].join('\n');
const sitemap = `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">
${ORDER.map((code) => `  <url>
    <loc>${pageUrl(code)}</loc>
    <lastmod>${STUDIO.updated}</lastmod>
${links}
  </url>`).join('\n')}
</urlset>
`;
writeFileSync(join(docs, 'sitemap.xml'), sitemap, 'utf8');
console.log('[OK] docs/sitemap.xml');
