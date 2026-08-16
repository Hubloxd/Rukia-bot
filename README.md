# Rukia-bar

Współbieżny bot muzyczny Discord napisany w języku Rust, wykorzystujący architekturę event-driven oraz asynchroniczne przetwarzanie strumieni audio.

Projekt skupia się na praktycznym wykorzystaniu:

* async/await
* stream processing
* reactive programming
* event-driven systems
* concurrent task scheduling
* audio pipeline architecture

---

# Funkcjonalności

* odtwarzanie muzyki z YouTube
* obsługa wielu kanałów głosowych jednocześnie
* współbieżne pobieranie utworów
* asynchroniczne przetwarzanie audio
* kolejki utworów dla wielu guildów
* event-driven command handling
* niezależne pipeline’y audio dla każdego voice channelu
* system workerów oparty o Tokio
* dynamiczne zarządzanie voice session
* obsługa komend Discord

---

# Architektura projektu

```text id="f9f0ru"
Discord Gateway
        │
        ▼
 Event Dispatcher
        │
 ┌──────┼──────────────┐
 ▼      ▼              ▼
Guild  Music Queue   Logging
State  Manager       System
 │
 ▼
Audio Download Workers
 │
 ▼
Audio Processing Pipeline
 │
 ▼
Voice Streaming Engine
 │
 ▼
Discord Voice Channels
```

---

# Model współbieżności

Projekt wykorzystuje:

* Tokio Runtime
* async/await
* message passing
* task scheduling
* bounded channels
* worker pools
* event-driven architecture

Każdy voice channel działa jako niezależny pipeline audio, umożliwiając jednoczesne odtwarzanie muzyki na wielu serwerach Discord.

---

# Technologie

* Rust
* Tokio
* Serenity
* Songbird
* ffmpeg
* yt-dlp

---

# Wymagania

## Docker (zalecane)

Na hoście wystarczą [Docker](https://docs.docker.com/get-docker/) i Docker Compose. Obraz zawiera m.in. **yt-dlp**, **ffmpeg** i binarkę bota — bez instalacji tych narzędzi lokalnie.

Projekt wymaga jawnej zależności `symphonia` z formatami (WebM/MKV, MP4, AAC…) — songbird ich sam nie włącza. Bez tego `!play` kończy się błędem `no suitable format reader found`.

## Lokalny development (opcjonalnie)

### macOS

```bash id="v9vlt0"
brew install cmake opus pkg-config ffmpeg yt-dlp
```

### Rust

https://rustup.rs

---

# Konfiguracja

## Klonowanie repozytorium

```bash id="5c5tfh"
git clone https://github.com/twoj-login/rukia-bar.git
cd rukia-bar
```

## Zmienne środowiskowe

Skopiuj szablon i wstaw token bota:

```bash
cp src/discord_token.env.example src/discord_token.env
```

```env id="6cax4f"
TOKEN=twoj_token_bota
```

Działa też nazwa `DISCORD_TOKEN`. Plik `src/discord_token.env` jest w `.gitignore` — nie commituj go.

---

# Uruchomienie

## Docker

```bash
docker compose up --build
```

W tle:

```bash
docker compose up --build -d
```

Logi: `docker compose logs -f`

**Discord voice a UDP:** `compose.yaml` używa `network_mode: host` (Linux). Bez tego bot może wejść na kanał (`!join`), ale **audio z `!play` nie dotrze** — klasyczny problem NAT w Dockerze.

**macOS (Docker Desktop):** `network_mode: host` nie przekazuje UDP tak jak na Linuxie. Jeśli `!join` działa, a `!play` milczy, uruchom bota lokalnie (`cargo run` + `brew install ffmpeg yt-dlp`) albo deployuj obraz na maszynie z Linuxem.

Katalog `/app` w kontenerze jest pusty z założenia — binarka leży w `/usr/local/bin/rukia-bar`, a token jest wstrzykiwany przez `env_file`.

## Lokalnie (cargo)

```bash id="1x8d3g"
cargo run
```

Wymaga `yt-dlp` i `ffmpeg` w `PATH` na hoście.

---

# Komendy

| Komenda        | Opis                                                       |
| -------------- | ---------------------------------------------------------- |
| `!join`        | Bot dołącza do kanału głosowego                            |
| `!play <url>`  | Odtwarzanie utworu z YouTube (kolejka per serwer)          |
| `!seek <czas>` | Przewinięcie bieżącego utworu (`45`, `1:30`, `1:05:20`)    |
| `!pause`       | Wstrzymanie / wznowienie odtwarzania                       |
| `!loop`        | Włączenie / wyłączenie pętli aktualnego utworu             |
| `!skip`        | Pominięcie aktualnego utworu                               |
| `!queue`       | Wyświetlenie kolejki                                       |
| `!leave`       | Opuszczenie kanału głosowego                               |

---

# Cele projektu

Projekt ma na celu praktyczne wykorzystanie zagadnień związanych z:

* programowaniem współbieżnym
* projektowaniem systemów reaktywnych
* stream processing
* zarządzaniem współdzielonym stanem
* przetwarzaniem zdarzeń
* komunikacją między taskami
* budową skalowalnych systemów async

---

# Inspiracje architektoniczne

Projekt inspirowany jest rozwiązaniami:

* Reactor Pattern
* RxJava
* Reactive Streams
* Actor Model
* Event Loop Architecture

---

# Status projektu

Działają: `!join`, `!play`, `!seek`, `!pause`, `!loop`, `!queue`, `!skip`, `!leave` — osobna kolejka i stan na każdą gildię (`src/guild/`, songbird `builtin-queue`).

W przygotowaniu: worker pool, zaawansowane zarządzanie kolejką.

## Test `!play`

1. `docker compose up --build` (Linux) lub `cargo run` (macOS / dev)
2. Na serwerze Discord: `!join` lub od razu `!play` (bot sam dołączy do Twojego kanału)
3. `!play https://www.youtube.com/watch?v=…` lub `!play nazwa utworu`
4. Przy błędzie streamu bot odpowie na kanale tekstowym; szczegóły: `docker compose logs -f`

---

# Licencja

MIT
