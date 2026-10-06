# cloudrs — pesquisa e plano inicial

Cliente desktop nativo de SoundCloud em Rust + GPUI.

## O que já existe (out/2026)

| Projeto | Stack | Observação |
|---|---|---|
| [fastcloud](https://github.com/COMF2222/fastcloud) | Tauri + React, core de áudio em Rust | Webview, não é UI nativa |
| [SoundCloud-Desktop](https://github.com/zxcloli666/SoundCloud-Desktop-EN) | Tauri 2 + React 19 | Popular, ~80–120 MB RAM |
| [Sonora](https://github.com/sonorahq/sonora) | Rust + GPUI | Multi-provedor (Spotify, YT Music...), GPL-3. Referência de arquitetura |
| [optionMusic](https://github.com/fireflylabss/optionMusic) | TUI + front GPUI | Tem SoundCloud como um dos provedores |
| [rust-player](https://github.com/jhoogstraat/rust-player) | GPUI + spotatui | Boa separação app/core/adapter |

Conclusão: não há um cliente **dedicado ao SoundCloud, 100% nativo em GPUI** e maduro.
Os dedicados são Tauri/webview; os GPUI são multi-serviço.

## Acesso à API

- **API oficial** (`api.soundcloud.com`): OAuth 2.1 + PKCE, token ~1h. Registro de app
  é manual/lento e exige conta Artist Pro. Stream: `/tracks/:urn/streams` → `hls_aac_160_url`
  (MP3/Opus foram descontinuados em 2025). Limite de 15k plays/24h por app.
- **API interna** (`api-v2.soundcloud.com`): usada pelo site; exige `client_id` extraído do
  JS do site (muda periodicamente). Mais completa (feed, likes, recomendações), mas é
  não-oficial e viola ToS — risco de quebrar a qualquer momento.

Estratégia: trait `SoundCloudApi` com dois backends (oficial e v2), para trocar sem mexer na UI.

## Arquitetura proposta (workspace)

```
crates/
  sc-api/      cliente HTTP (reqwest + serde), modelos, auth, paginação
  sc-audio/    player: HLS (m3u8-rs) → decode AAC (symphonia) → saída (cpal/rodio), fila, seek
  sc-core/     estado da app, fila, cache, persistência (sqlite/redb), comandos
apps/
  cloudrs/     UI GPUI (+ gpui-component), sem lógica de negócio
```

- Áudio em thread própria, comunicação via canais (`crossbeam`/`flume`), UI recebe snapshots.
- Tokio para rede; GPUI tem executor próprio — bridge via canais.
- GPUI: crates.io oficial está congelado (0.2.2). Usar git pin do repo do Zed ou
  `gpui-component`/GPUI Kit, que acompanha upstream.
- Integrações do SO: `souvlaki` (MPRIS / Media Keys / SMTC), `keyring` para tokens.

## Roadmap

1. **MVP**: login, buscar faixa, tocar (HLS AAC), play/pause/seek/volume.
2. Fila, likes, playlists, histórico, waveform (o SoundCloud fornece `waveform_url`).
3. Feed/stream, comentários com timestamp, cache offline, mini-player, media keys.
4. Equalizador, normalização, crossfade, Discord RPC, temas.
