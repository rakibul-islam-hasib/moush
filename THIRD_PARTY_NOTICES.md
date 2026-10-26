# Third-party notices

Moush is MIT-licensed, but it uses third-party software, fonts and media. Their licenses
apply to those components.

## Runtime and build dependencies

- Rust crates: see `Cargo.lock` and each crate's `license` field.
- npm packages: see `package-lock.json` and each package's `license` field.
- Tauri, React, Vite, TypeScript, Rust and Windows are trademarks of their respective
  owners.

## Fonts

- **Geist** and **Geist Mono** — SIL Open Font License 1.1, © Vercel. Bundled through
  `@fontsource-variable/geist` and `@fontsource-variable/geist-mono`; the license ships with
  those packages and is available at <https://github.com/vercel/geist-font>.

## Launch film and website media

- Music: "Happy Beats / Business Moves Vol. 1" by Sascha Ende,
  [ende.app](https://ende.app/en), [CC BY 4.0](https://ende.app/en/standard-license).
- Sound effects: [Kenney](https://kenney.nl) (CC0), plus whooshes and a riser synthesised
  for the film.
- Icons: [Icons8](https://icons8.com) (keyboard, mouse, email, USB drive, Rust, React,
  TypeScript, Vite, Windows 11).
- Voice: [ElevenLabs](https://elevenlabs.io), used to generate the narration.

The rendered film and poster are in `website/assets/video/`. The editable HyperFrames
composition, music, sound effects and narration are kept in the gitignored `brag-output/`
working directory and are not part of the source distribution.
