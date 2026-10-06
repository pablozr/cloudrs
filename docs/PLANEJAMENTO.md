# cloudrs — Planejamento

Cliente desktop **open source, nativo e leve** para o SoundCloud, escrito em Rust com GPUI.

> Pesquisa de mercado e contexto: [`pesquisa-e-plano.md`](./pesquisa-e-plano.md)

---

## 1. Visão

**Objetivo:** ouvir SoundCloud no desktop com a fluidez de um app nativo, sem webview ou Electron:
- abrir em menos de 300 ms;
- usar menos de 100 MB de RAM tocando;
- rodar a 120 fps.

**Público:** quem ouve SoundCloud o dia todo (mixes longos, DJ sets, faixas underground) e quer:
- integração com o sistema (teclas de mídia, MPRIS, notificações);
- fila de verdade;
- cache;
- atalhos de teclado.

### Objetivos (v1.0)
- Buscar e tocar faixas, playlists, álbuns e perfis.
- Login com a conta do usuário para acessar curtidas, playlists, feed e quem ele segue.
- Fila, histórico, shuffle, repeat e retomar a sessão ao reabrir o app.
- Waveform do SoundCloud na barra de progresso, com comentários marcados no tempo da música.
- Teclas de mídia e controles do SO: MPRIS no Linux, Now Playing no macOS e SMTC no Windows.
- Linux, Windows e macOS.

### Não-objetivos (por enquanto)
- Upload de faixas e ferramentas de artista.
- Download para arquivo e "rip". Fica fora de propósito, para não dar motivo de DMCA no repositório.
- Mobile e web.
- Outros serviços (Spotify, YouTube etc.). O foco é **só SoundCloud**, e esse é o nosso diferencial.

---

## 2. Decisões de arquitetura (resumo)

| # | Decisão | Motivo |
|---|---|---|
| D1 | **GPUI via `gpui-component` 0.7.x**, que fixa `gpui-pre =0.3.8` no crates.io | O `gpui` oficial no crates.io está congelado na 0.2.2. O gpui-component traz mais de 75 componentes (virtual list, inputs, dock, temas) e é usado em produção pela Longbridge |
| D2 | **API interna `api-v2.soundcloud.com`**, atrás de uma `trait` | Grátis e sem aprovação. A API oficial exige processo manual e conta paga. A trait permite trocar de backend depois |
| D3 | **Pipeline de áudio próprio**: `symphonia` (decode) + `cpal` (saída) | Controle total de buffer, seek, gapless e EQ. O `rodio` é simples, mas limita gapless e seek em streams |
| D4 | **Tokio** para rede e **thread dedicada** para áudio. Comunicação por canais (`flume`) | O GPUI tem executor próprio. Áudio nunca pode depender da UI nem da rede |
| D5 | **Estado central em `sc-core`**. A UI só lê snapshots e envia comandos | Permite testar a lógica sem UI e ter uma UI "fake" para desenvolvimento |
| D6 | **Persistência em SQLite** (`rusqlite`) + cache de arquivos em disco | Histórico, sessão, cache de metadados e de imagens |
| D7 | **Tokens no keyring do SO** (`keyring`) | Nunca guardar o `oauth_token` em texto puro |

Cada decisão vira um ADR curto em `docs/adr/` quando for implementada.

---

## 3. Estrutura do workspace

```
cloudrs/
├── Cargo.toml              # [workspace], versões centralizadas em [workspace.dependencies]
├── crates/
│   ├── sc-api/             # cliente HTTP da api-v2: modelos, client_id, auth, paginação
│   ├── sc-audio/           # engine de áudio: HLS/progressive → decode → saída
│   ├── sc-core/            # estado da app, fila, comandos/eventos, persistência, cache
│   └── sc-platform/        # integrações do SO: media keys, MPRIS, keyring, notificações
├── apps/
│   └── cloudrs/            # binário GPUI: telas, componentes, tema
├── assets/                 # ícones (SVG), fontes, logo
└── docs/
    ├── PLANEJAMENTO.md
    └── adr/
```

**Dependências entre crates** (de cima para baixo, sem ciclos):

```
apps/cloudrs ──► sc-core ──► sc-api
     │              ├──────► sc-audio
     └──────────────┴──────► sc-platform
```

`sc-api` e `sc-audio` não se conhecem. Quem passa a URL do stream da API para o player é o `sc-core`.

---

## 4. `sc-api` — cliente SoundCloud

### 4.1 `client_id`
1. `GET https://soundcloud.com/` → achar as tags `<script src="https://a-v2.sndcdn.com/assets/*.js">`.
2. Baixar os scripts, **do último para o primeiro** (o `client_id` costuma estar nos últimos), e aplicar a regex `client_id\s*[:=]\s*"([a-zA-Z0-9]{32})"`.
3. Guardar o `client_id` em cache (SQLite) com a data.
4. Se alguma chamada responder **401/403**, extrair de novo **uma vez** e repetir a requisição.
5. Permitir sobrescrever o valor via config, para debug.

### 4.2 Autenticação do usuário
- O app não tem OAuth próprio na v2. A opção principal é o usuário colar o `oauth_token`, com um guia passo a passo mostrando onde achar o cookie `oauth_token` no navegador.
- Opção futura: uma janela de login (`wry`/webview mínima) que lê o cookie depois do login. Fica só como "nice to have", porque traz a webview de volta.
- Cabeçalho das requisições: `Authorization: OAuth <token>`.
- Validar o token com `GET /me` e guardar no keyring.

### 4.3 Endpoints iniciais (todos com `?client_id=`)

| Uso | Endpoint |
|---|---|
| Busca | `/search?q=`, `/search/tracks`, `/search/users`, `/search/playlists`, `/search/albums` |
| Resolver URL colada | `/resolve?url=https://soundcloud.com/...` |
| Faixa | `/tracks/{id}`, `/tracks?ids=1,2,3` (em lote, até ~50) |
| Relacionadas | `/tracks/{id}/related` |
| Comentários | `/tracks/{id}/comments?threaded=0` |
| Playlist | `/playlists/{id}` (as faixas vêm parcialmente: completar via `/tracks?ids=`) |
| Usuário | `/users/{id}`, `/users/{id}/tracks`, `/users/{id}/playlists`, `/users/{id}/likes` |
| Logado | `/me`, `/me/library/all`, `/users/{me}/track_likes`, `/stream` (feed), `/me/followings` |
| Ações | `PUT/DELETE /users/{me}/track_likes/{id}`, follow/unfollow |
| Stream | URL de `media.transcodings[i].url` + `client_id` + `track_authorization` → `{ "url": "<m3u8 ou mp3>" }` |

### 4.4 Detalhes de implementação
- **Paginação:** usar `linked_partitioning=1` e seguir o `next_href`. Expor como `Stream<Item = Result<Page<T>>>` ou como um `Paginator<T>` simples.
- **Modelos:** `serde` com `#[serde(default)]` e campos opcionais à vontade, porque a API muda sem aviso. **Testes com fixtures JSON reais** em `crates/sc-api/tests/fixtures/`.
- **Erros:** `thiserror`, com os casos `Unauthorized`, `RateLimited { retry_after }`, `NotFound`, `GeoBlocked`, `Network`, `Decode`.
- **Rate limit:** backoff exponencial em 429, e limite de concorrência com um semáforo de cerca de 4 requisições.
- **Artwork:** `artwork_url` vem em `-large` (100px). Trocar para `-t500x500` ou `-t300x300` conforme o uso.
- **Trait pública:**

```rust
#[async_trait]
pub trait SoundCloudApi: Send + Sync {
    async fn search_tracks(&self, q: &str, page: PageReq) -> Result<Page<Track>>;
    async fn resolve(&self, url: &str) -> Result<Resource>;
    async fn track(&self, id: TrackId) -> Result<Track>;
    async fn stream_url(&self, track: &Track) -> Result<StreamSource>;
    async fn me(&self) -> Result<User>;
    // ...
}
```

---

## 5. `sc-audio` — engine de áudio

### 5.1 Escolha do formato
O SoundCloud oferece vários `transcodings` por faixa. Ordem de preferência, com fallback:
1. `hls` + `audio/mp4; codecs="mp4a.40.2"` (AAC 160k, que é o padrão atual)
2. `hls` + AAC 96k
3. `progressive` + `audio/mpeg` (MP3 128k), se ainda existir
4. `hls` + `audio/mpeg` / `audio/ogg; codecs="opus"` (legado)

Transcodings criptografados (`encrypted-hls`, `ctr-encrypted-hls`, `cbc-encrypted-hls`) e faixas `snipped` (prévia de 30s do Go+) devem ser **ignorados**, mostrando um aviso na UI.

> ⚠️ O **M0 (spike)** existe justamente para confirmar, com faixas reais, quais formatos aparecem hoje e qual container os segmentos AAC usam (fMP4 ou ADTS).

### 5.2 Pipeline

```
[fetcher (tokio)]  m3u8 → baixa segmentos à frente (~30s) → ring buffer de bytes
        │
[decoder thread]   symphonia (isomp4/adts + aac | mp3 | ogg/opus) → PCM f32
        │          resample (rubato) para a taxa da saída, volume, EQ (futuro)
        │
[cpal callback]    ring buffer de amostras (rtrb, lock-free) → placa de som
```

- **Seek no HLS:** usar o `#EXTINF` de cada segmento para calcular em qual segmento cair, baixar a partir dele e descartar amostras até o tempo exato.
- **Gapless / pré-carga:** quando faltarem cerca de 20s, a faixa seguinte já começa a ser resolvida e baixada.
- **Expiração da URL:** as URLs de stream expiram depois de alguns minutos. Em pausa longa ou seek, pedir a URL de novo ao `sc-core` com um callback/canal de "refresh".
- **Eventos emitidos:** `Position(Duration)` a cerca de 10 Hz, `Buffering(bool)`, `TrackEnded`, `Error(..)`, `NearEnd`.
- **Comandos recebidos:** `Load(Source)`, `Play`, `Pause`, `Seek(Duration)`, `SetVolume(f32)`, `Preload(Source)`, `Stop`.
- **Dispositivo de saída:** listar e trocar via cpal, e se recuperar quando o dispositivo some (fone desconectado).

---

## 6. `sc-core` — estado e regras

- **`AppState`**:
  - sessão (usuário, tokens);
  - `PlayerState` (faixa atual, posição, volume, status);
  - `Queue`;
  - caches (faixas e usuários por id).
- **Fila:**
  - `Vec<TrackId>` com índice atual;
  - contexto de origem (playlist X, busca Y, curtidas);
  - shuffle com ordem preservada (para desfazer);
  - repeat off/one/all;
  - "tocar a seguir" e "adicionar ao fim".
- **Autoplay:** quando a fila acaba, buscar `/tracks/{id}/related` e continuar tocando, como faz o site.
- **Persistência (SQLite em `dirs::data_dir()/cloudrs/`):**
  - `session` (fila, posição, volume);
  - `history`;
  - `cache_tracks` (JSON + TTL);
  - `settings`.
- **Cache de imagens:** arquivos em `dirs::cache_dir()/cloudrs/img/`, com a chave sendo o hash da URL e limite de tamanho com LRU.
- **API para a UI:** `Command` (enum) entra e `Event` / snapshot sai. A UI nunca chama `sc-api` direto.

---

## 7. UI (`apps/cloudrs`)

### 7.1 Layout

```
┌──────────┬──────────────────────────────────────────────┐
│ Sidebar  │  Conteúdo (rotas)                            │
│ ─ Início │                                              │
│ ─ Feed   │                                              │
│ ─ Buscar │                                              │
│ ─ Curtid.│                                              │
│ ─ Playl. │                                              │
│  ...     │                                              │
├──────────┴──────────────────────────────────────────────┤
│ ▶ ⏮ ⏭  [▁▃▅▇▅▃▁▃▅▇ waveform ▇▅▃▁]  01:23 / 58:10  🔊 ☰ │
└─────────────────────────────────────────────────────────┘
```

### 7.2 Telas
1. **Buscar:** campo com debounce de 300 ms, abas (Tudo / Faixas / Pessoas / Playlists) e scroll infinito com virtual list.
2. **Faixa:** artwork, waveform grande, descrição, comentários, relacionadas.
3. **Playlist / Álbum:** cabeçalho e lista de faixas.
4. **Perfil:** cabeçalho, abas (Faixas / Playlists / Curtidas / Reposts) e botão de seguir.
5. **Curtidas e Biblioteca** (logado).
6. **Feed** (logado).
7. **Fila:** painel lateral com drag & drop para reordenar.
8. **Configurações:** conta/token, dispositivo de áudio, tema, cache, atalhos.

### 7.3 Componentes próprios
- `Waveform`: desenha a partir do `waveform_url` (JSON com cerca de 1800 amostras), com cor de progresso e clique ou arraste para seek.
- `TrackRow`, `TrackCard`, `UserCard`, `PlaylistCard`.
- `PlayerBar`.
- `AsyncImage`: carrega do cache e mostra placeholder.

### 7.4 Atalhos
| Tecla | Ação |
|---|---|
| `Espaço` | play/pause |
| `←` / `→` | ±5s |
| `Shift+←/→` | anterior/próxima |
| `Ctrl+L` | curtir |
| `Ctrl+K` / `/` | buscar |
| `Ctrl+V` em qualquer lugar | resolver a URL colada do SoundCloud e tocar |

---

## 8. `sc-platform` — integrações do SO
- **Teclas de mídia e "Now Playing":** `souvlaki` (MPRIS / macOS / SMTC).
- **Tokens:** `keyring`.
- **Notificações de nova faixa:** `notify-rust` (Linux/Windows), opcional.
- **Discord Rich Presence:** `discord-rich-presence`, opcional e desligado por padrão (M5).
- **Ícone de bandeja:** `tray-icon` (M5).

---

## 9. Marcos (milestones)

### M0 — Spike técnico (validar riscos) · ~1 semana
- [ ] Workspace Cargo + CI (fmt, clippy, test) no GitHub Actions para Linux, macOS e Windows.
- [ ] `sc-api`: extrair o `client_id` e fazer `search/tracks` imprimindo no terminal (exemplo `cargo run --example search`).
- [ ] `sc-audio`: tocar uma faixa HLS AAC de ponta a ponta, sem UI (`cargo run --example play <url>`).
- [ ] Janela GPUI "hello" com gpui-component compilando nas 3 plataformas.
- [ ] Documentar os achados (formatos, container, expiração de URL) em `docs/adr/0001-*.md`.

### M1 — MVP tocável
- [ ] Buscar faixas → lista → clicar e tocar.
- [ ] PlayerBar: play/pause, seek, volume, tempo.
- [ ] Waveform na PlayerBar.
- [ ] Colar a URL do SoundCloud e tocar.
- [ ] Artwork com cache.

### M2 — Navegação
- [ ] Telas de Faixa, Perfil, Playlist e Álbum.
- [ ] Fila completa (próxima/anterior, tocar a seguir, shuffle, repeat, reordenar).
- [ ] Autoplay com relacionadas.
- [ ] Histórico e retomada de sessão.

### M3 — Conta do usuário
- [ ] Login via token + keyring.
- [ ] Curtidas, Biblioteca, Feed, Seguindo.
- [ ] Curtir/descurtir, seguir/deixar de seguir.

### M4 — Integração com o desktop
- [ ] MPRIS / Now Playing / SMTC.
- [ ] Atalhos de teclado completos.
- [ ] Seleção de dispositivo de áudio e recuperação de desconexão.
- [ ] Comentários na waveform.
- [ ] Configurações + temas claro/escuro.

### M5 — Polimento e release 0.1
- [ ] Gapless, normalização de volume, equalizador.
- [ ] Mini-player, bandeja, Discord RPC.
- [ ] Empacotamento: `.AppImage`/`.deb`/Flatpak, `.dmg`, `.msi` (via `cargo-dist` ou `cargo-packager`).
- [ ] Site/README com GIFs e página de download.

---

## 10. Qualidade e processo
- **Rust estável**, edition 2024, MSRV fixada em `rust-toolchain.toml`.
- **CI:** `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` nas 3 plataformas, e `cargo deny` (licenças e advisories).
- **Testes:**
  - `sc-api`: fixtures JSON + `wiremock`. Nada de rede real no CI.
  - `sc-audio`: decodificar arquivos de teste pequenos em `tests/assets/`.
  - `sc-core`: testes da fila, shuffle e retomada (lógica pura).
  - Um job opcional e manual de "smoke test" contra o SoundCloud real, para detectar quebras da API.
- **Logs:** `tracing` + `tracing-subscriber`, com `RUST_LOG=cloudrs=debug`.
- **Commits:** Conventional Commits (`feat:`, `fix:`, `docs:`...), para gerar changelog automático.
- **Issues:** templates de bug/feature e labels `good first issue` para atrair contribuidores.

---

## 11. Riscos

| Risco | Impacto | Mitigação |
|---|---|---|
| SoundCloud muda a `api-v2` ou o `client_id` | Alto | Reextração automática, modelos tolerantes, smoke test semanal, trait para trocar de backend |
| Streams passam a ser só criptografados (DRM) | Alto | Monitorar. Não contornar DRM, por motivo legal. Avisar o usuário quando a faixa não puder tocar |
| API do GPUI muda (é pré-1.0) | Médio | Versão fixada pelo gpui-component. Atualizar em PRs dedicados |
| Pedido de remoção (DMCA/ToS) | Médio | Não oferecer download, não remover anúncios ou paywall, não contornar o Go+. Aviso claro de "cliente não oficial" |
| Áudio com stutter | Médio | Thread dedicada, buffers lock-free, nunca alocar no callback do cpal |

---

## 12. Open source
- **Licença:** decidir entre
  - **MIT OR Apache-2.0** (padrão do ecossistema Rust, mais permissiva);
  - **GPL-3.0** (garante que forks continuem abertos; o Sonora usa GPL-3).
- **Aviso no README:**
  > cloudrs é um cliente não oficial e não é afiliado, endossado ou patrocinado pela SoundCloud Global Limited & Co. KG.
- `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md` e templates de issue/PR a partir do M1.
- Nome e logo: evitar o logo e a cor exata do SoundCloud, por causa da marca registrada.

---

## 13. Próximo passo

Começar o **M0**: criar o workspace e os três exemplos (`search`, `play`, `hello-window`).
