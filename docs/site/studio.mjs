// What the project page says about this studio beyond its text: the release it offers, the features
// its tiles show (places in each language's features list, with the screenshot each shows), and the answers it points to.
export const STUDIO = {
  id: 'ace',
  name: 'ACE-Step Studio',
  repo: 'https://github.com/timoncool/ACE-Step-Studio',
  site: 'https://timoncool.github.io/ACE-Step-Studio/',
  version: '3.4.0',
  installerMB: 451.31,
  updated: '2026-10-01',
  bento: [
    { feature: 7, shot: '05-lora' },
    { feature: 1, shot: '02-simple' },
    { feature: 8, shot: '08-training' },
    { feature: 11, shot: '09-midi' },
    { feature: 17, shot: '15-covers' },
    { feature: 13, shot: '11-agent' },
  ],
  faq: { lora: 8, mcp: 13 },
  arch: "React UI ─┐\n          ├─ ACE-Step-Studio.exe   (Tauri window + Rust/Axum service)\nRust axum ┘        │\n                   └─ acestep.cpp `ace-server`   (C++/GGML, GGUF)",
};
