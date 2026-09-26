// Renders the project page into docs/index.html (English) and docs/<lang>.html.
// Every page is plain HTML with its own language's text, so search engines and
// readers without JavaScript see the same page.
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ORDER, SAMPLES, SETS, STRINGS } from '../docs/site/strings.mjs';

const REPO = 'https://github.com/timoncool/ACE-Step-Studio';
const SITE = 'https://timoncool.github.io/ACE-Step-Studio/';
const docs = join(dirname(fileURLToPath(import.meta.url)), '..', 'docs');
const css = readFileSync(join(docs, 'site', 'page.css'), 'utf8');

const escape = (text) => String(text)
  .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const file = (lang) => (lang === 'en' ? 'index.html' : `${lang}.html`);
const cards = (list) => list
  .map(([title, body]) => `      <div class="card"><h3>${escape(title)}</h3><p>${escape(body)}</p></div>`)
  .join('\n');

function page(lang) {
  const s = STRINGS[lang];
  for (const key of Object.keys(STRINGS.en)) {
    if (!(key in s)) throw new Error(`[ERROR] ${lang} is missing "${key}"`);
  }
  const alternates = ORDER
    .map((code) => `<link rel="alternate" hreflang="${code}" href="${SITE}${code === 'en' ? '' : file(code)}" />`)
    .join('\n');
  const nav = ORDER
    .map((code) => `      <a href="${file(code)}"${code === lang ? ' aria-current="page"' : ''}>${escape(STRINGS[code].label)}</a>`)
    .join('\n');
  const shots = s.shots
    .map(([name, caption]) => `      <figure><img src="screenshots/${lang}-${name}.png" alt="${escape(caption)}" loading="lazy" /><figcaption>${escape(caption)}</figcaption></figure>`)
    .join('\n');
  const samples = SAMPLES
    .map((sample) => `      <figure class="sample"><figcaption><b>${escape(sample.title)}</b><span>${escape(sample.note[lang])}</span><code>${escape(sample.style)}</code></figcaption><audio controls preload="none" src="samples/${sample.file}"></audio></figure>`)
    .join('\n');
  const sets = SETS
    .map((set) => `          <tr><td>${set.vram}</td><td>${escape(s[set.key])}</td><td>${set.size}</td></tr>`)
    .join('\n');
  const inside = s.inside
    .map((row) => `          <tr>${row.map((cell) => `<td>${escape(cell)}</td>`).join('')}</tr>`)
    .join('\n');
  const steps = s.steps
    .map(([title, body]) => `      <li><b>${escape(title)} —</b> <span>${escape(body)}</span></li>`)
    .join('\n');

  return `<!doctype html>
<html lang="${lang}">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>${escape(s.title)}</title>
<meta name="description" content="${escape(s.description)}" />
<link rel="icon" type="image/png" href="logo.png" />
<meta property="og:title" content="ACE-Step Studio" />
<meta property="og:description" content="${escape(s.description)}" />
<meta property="og:type" content="website" />
<meta property="og:image" content="${SITE}screenshots/${lang}-01-create.png" />
${alternates}
<style>
${css.trimEnd()}
</style>
</head>
<body>
<header>
  <div class="wrap bar">
    <img class="mark" src="logo.png" alt="" />
    <div class="name">ACE-Step Studio</div>
    <nav class="langs" aria-label="Language">
${nav}
    </nav>
  </div>
</header>

<main class="wrap">
  <img src="logo.png" alt="ACE-Step Studio" width="96" height="96" style="border-radius:22px;margin-top:36px" />
  <h1>${escape(s.heroTitle)}</h1>
  <p class="lead">${escape(s.heroLead)}</p>
  <div class="cta">
    <a class="btn primary" href="${REPO}/releases/latest">${escape(s.ctaDownload)}</a>
    <a class="btn" href="${REPO}">${escape(s.ctaSource)}</a>
    <a class="btn" href="${REPO}/blob/main/DONATE.md">${escape(s.ctaDonate)}</a>
  </div>
  <p class="note">${escape(s.ctaNote)}</p>

  <figure class="hero-shot">
    <img src="screenshots/${lang}-01-create.png" alt="${escape(s.shots[0][1])}" loading="eager" />
  </figure>

  <section>
    <h2>${escape(s.featuresTitle)}</h2>
    <p class="sub">${escape(s.featuresSub)}</p>
    <div class="grid">
${cards(s.features)}
    </div>
  </section>

  <section id="samples">
    <h2>${escape(s.samplesTitle)}</h2>
    <p class="sub">${escape(s.samplesSub)}</p>
    <div class="samples">
${samples}
    </div>
  </section>

  <section>
    <h2>${escape(s.shotsTitle)}</h2>
    <p class="sub">${escape(s.shotsSub)}</p>
    <div class="grid-shots">
${shots}
    </div>
  </section>

  <section>
    <h2>${escape(s.modelsTitle)}</h2>
    <p class="sub">${escape(s.modelsSub)}</p>
    <div class="scroll">
      <table>
        <thead><tr><th>${escape(s.modelsGpu)}</th><th>${escape(s.modelsSet)}</th><th>${escape(s.modelsSize)}</th></tr></thead>
        <tbody>
${sets}
        </tbody>
      </table>
    </div>
    <p class="note">${escape(s.modelsNote)}</p>
  </section>

  <section>
    <h2>${escape(s.startTitle)}</h2>
    <ol class="steps">
${steps}
    </ol>
  </section>

  <section>
    <h2>${escape(s.insideTitle)}</h2>
    <p class="sub">${escape(s.insideSub)}</p>
    <div class="scroll">
      <table>
        <thead><tr><th>${escape(s.insidePart)}</th><th>${escape(s.insideRuns)}</th><th>${escape(s.insideSize)}</th></tr></thead>
        <tbody>
${inside}
        </tbody>
      </table>
    </div>
  </section>

  <section>
    <h2>${escape(s.archTitle)}</h2>
    <p class="sub">${escape(s.archSub)}</p>
<pre>React UI ─┐
          ├─ ACE-Step-Studio.exe   (Tauri window + Rust/Axum service)
Rust axum ┘        │
                   └─ acestep.cpp \`ace-server\`   (C++/GGML, GGUF)</pre>
  </section>

  <section>
    <h2>${escape(s.privacyTitle)}</h2>
    <p class="sub">${escape(s.privacySub)}</p>
    <div class="grid">
${cards(s.privacy)}
    </div>
  </section>

  <section>
    <h2>${escape(s.authorTitle)}</h2>
    <p class="sub">${escape(s.authorSub)}</p>
    <div class="pills">
      <a class="pill" href="https://github.com/timoncool">GitHub · @timoncool</a>
      <a class="pill" href="https://t.me/nerual_dreming">Telegram · @nerual_dreming</a>
      <a class="pill" href="https://t.me/neuroport">Telegram · @neuroport</a>
      <a class="pill" href="https://artgeneration.me">ArtGeneration.me</a>
      <a class="pill" href="https://boosty.to/neuro_art">Boosty</a>
      <a class="pill" href="https://dalink.to/nerual_dreming">Card / PayPal</a>
    </div>
  </section>

  <footer>
    <p>${escape(s.footerLicense)}</p>
    <p><a href="${REPO}/blob/main/CHANGELOG.md">${escape(s.footerChanges)}</a> ·
       <a href="${REPO}/issues">${escape(s.footerIssues)}</a></p>
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
  const image = event.target.closest('.grid-shots img, .hero-shot img');
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
</script>
</body>
</html>
`;
}

for (const lang of ORDER) {
  writeFileSync(join(docs, file(lang)), page(lang), 'utf8');
  console.log(`[OK] docs/${file(lang)}`);
}
