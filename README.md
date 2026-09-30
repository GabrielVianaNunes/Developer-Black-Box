# Developer Black Box

**English** | [Português (Brasil)](#português-brasil)

A local "black box" for **Windows 11**: it records technical process events (start, exit, CPU,
memory, crashes and hangs), preserves incident evidence and helps you investigate software problems.
**Privacy first:** everything stays on your computer, encrypted, with no cloud, and the app never
captures passwords, typed text, page content, window titles, URLs, file paths or command lines.

## What it does

- **Tray icon** (a black cube with a light beside it): green only when recording is actually active;
  red when paused, suspended by privacy rules or failing. The tooltip and the window explain why.
- **Pause and resume** from the tray, even with the window closed. Resuming never bypasses the
  Privacy Guard.
- **Privacy Guard:** suspends recording when a browser or password manager is in the foreground, the
  session is locked or the detector is unavailable. The absence of a signal is never treated as "safe".
- **Incidents:** manual capture, sustained CPU, high memory, crashes and hangs (Windows Event Log),
  with evidence from the window before and after.
- **Dashboard:** overview, activity with filters, processes, incidents (timeline, notes, export),
  privacy, storage and integrity verification.
- **Test mode:** a temporary, revocable authorization to collect technical data from a browser, so you
  can test a web application of your own. Page content, forms and requests are never recorded.
- **Open with Windows** (optional): starts hidden in the tray and stays paused.
- **English and Portuguese (Brazil):** every screen, the tray menu and the messages are available in
  both languages. Switch at any time with the language selector at the top of the window; the choice is
  remembered. On first run the app follows your Windows language. The installer also follows it.

## Installation (use it on your PC)

1. Download the installer `Developer-Black-Box_<version>_x64-setup.exe` from the **Releases** tab of
   this repository (and `SHA256SUMS.txt` if you want to verify the file).
2. Run the installer. It installs **for your user only** (no administrator rights needed).
3. The installer is **not digitally signed**, so Windows SmartScreen may warn "Windows protected your
   PC". That is expected for a personal project without a code-signing certificate: choose "More info"
   > "Run anyway" if you trust the file. To check that it is the same file as the Release, compare
   the hash:

```powershell
(Get-FileHash ".\Developer-Black-Box_0.1.0_x64-setup.exe" -Algorithm SHA256).Hash.ToLower()
```

On first run the app **starts paused**: nothing is recorded until you click "Resume recording" (or turn
on "Start recording when the app opens" under Privacy). To uninstall, use Windows "Installed apps".
Your recorded data is **not** deleted: use "Delete everything" in the Storage tab before uninstalling,
or delete `%LOCALAPPDATA%\DeveloperBlackBox`.

## Building it yourself

Requirements: Windows 11, [Rust](https://rustup.rs) + Visual Studio Build Tools (C++), Node 22+ and
WebView2 (already included in Windows 11).

```bash
npm install
npx tauri build                            # builds the installer (NSIS target set in tauri.conf.json)
# installer: target/release/bundle/nsis/
```

Executable only, without an installer:

```bash
npm run build && cargo build -p bb-app --release --features tauri/custom-protocol
# target/release/developer-blackbox.exe
```

The first time you build the installer, Tauri downloads the NSIS tooling (from GitHub).

### Tests

```bash
cargo test --workspace   # all Rust tests, including tests/privacy/
npm test                 # tests for the repository safety scripts
npm run check:repo       # no sensitive files tracked + secret scanner
```

The privacy tests use **synthetic data only** and cover, among other things: manual pause, resume that
respects the Guard, excluded apps, temporary authorizations, encryption on disk, export with
re-filtering, crash recovery and the absence of network libraries in the core.

### Publishing a Release (maintainer)

The workflow `.github/workflows/release.yml` runs the tests, builds the installer and attaches it to
the Release when you create a tag (it has not been run on GitHub yet; try it with "Run workflow" first):

```bash
git tag v0.1.0 && git push origin v0.1.0
```

Before publishing, run `npm run check:repo` and `npm run check:history` (they look for sensitive files
and secrets in the repository and in its whole history) and, preferably, a dedicated scanner such as
`gitleaks`.

## Where the data lives

`%LOCALAPPDATA%\DeveloperBlackBox\`: `key.bin` (DPAPI-protected key), `meta.db`, `recorder\` (encrypted
segments) and `exports\`. None of it lives in the repository and all of it is in `.gitignore`.

## How privacy is guaranteed

- **Closed schema:** events only have numeric fields, the PID and the **executable name**. There is no
  free-text field, so there is nowhere for a secret to go.
- **Every event goes through the Privacy Guard** before reaching the recorder (the event type can only
  be created by the Guard).
- **No reconstruction:** what happens during a pause or block is never recorded afterwards, including
  crashes logged by Windows in that interval.
- **Encryption at rest:** events and the sensitive database fields use AES-256-GCM, with a key protected
  by DPAPI (tied to your Windows account). Deleted content is overwritten in the file.
- **Re-filtered export:** it applies today's privacy rules again and never includes notes.
- **No cloud and no telemetry.** The core does not depend on any network library.

## Limitations

- A power loss or a Windows crash can lose the last events still in the disk cache (a crash of the app
  process alone loses nothing).
- Protected or elevated processes are not seen.
- Protection depends on your Windows account: whoever uses your signed-in account can use the key.
- The export file is **not encrypted** (the interface warns about it).
- Metadata such as the number of incidents and their times sit in the database unencrypted (only app
  names, notes and settings are encrypted).
- Sensitive-context detection is per foreground application, not per password field.
- ETW is not implemented (the Windows Application log and process snapshots cover crashes, hangs and
  resources).
- The tray is verified by tests only at the logic level; the visual result is checked manually.

## Structure

```
crates/bb-core       events, state machine, Privacy Guard and test-mode authorizations
crates/bb-recorder   encrypted segment recording, hash chain, retention, recovery, DPAPI
crates/bb-collector  processes, metrics, Windows context, Event Log, start with Windows
crates/bb-store      SQLite (incidents, notes, settings) with sensitive fields encrypted
crates/bb-query      read-only queries and re-filtered export
crates/bb-engine     collection cycle, incidents, settings and export
crates/bb-tray       icon, texts and tray menu rules
src-tauri            Tauri application (tray, window and commands)
src                  React + TypeScript interface (src/i18n: English and Portuguese dictionaries)
tests/privacy        privacy and security tests (synthetic data)
scripts              repository safety checks
```

## License

[PolyForm Noncommercial 1.0.0](LICENSE): the code is public for study and noncommercial use.
Commercial use is not permitted. It is not an open source license in the OSI sense. The license file
also includes an unofficial Portuguese translation; the English text is the one that governs.

---

# Português (Brasil)

[English](#developer-black-box) | **Português (Brasil)**

Caixa-preta local para o **Windows 11**: registra eventos técnicos de processos (início, fim, CPU,
memória, falhas e travamentos), preserva evidências de incidentes e ajuda a investigar problemas de
software. **Privacidade em primeiro lugar:** tudo fica no seu computador, cifrado, sem nuvem, e o app
nunca captura senhas, texto digitado, conteúdo de páginas, títulos de janela, URLs, caminhos ou linhas
de comando.

## O que ele faz

- **Ícone na bandeja** (um cubo preto com uma luz ao lado): verde só quando a gravação está de fato
  ativa; vermelho quando pausada, suspensa por privacidade ou com falha. O tooltip e a janela explicam
  o motivo.
- **Pausa e retomada** pela bandeja, mesmo com a janela fechada. A retomada nunca ignora o Privacy Guard.
- **Privacy Guard:** suspende a gravação com navegadores e gerenciadores de senha em primeiro plano,
  sessão bloqueada ou detector indisponível. Ausência de sinal nunca é tratada como "seguro".
- **Incidentes:** captura manual, CPU sustentada, memória alta, falhas e travamentos (Event Log do
  Windows), com evidências da janela anterior e posterior.
- **Painel:** visão geral, atividade com filtros, processos, incidentes (linha do tempo, anotações,
  exportação), privacidade, armazenamento e verificação de integridade.
- **Modo de teste:** autorização temporária e revogável da coleta técnica de um navegador, para testar
  uma aplicação web sua. Nunca há conteúdo de páginas, formulários ou requisições.
- **Abrir com o Windows** (opcional): abre escondido na bandeja e continua pausado.
- **Inglês e português (Brasil):** todas as telas, o menu da bandeja e as mensagens existem nos dois
  idiomas. Troque a qualquer momento no seletor de idioma no topo da janela; a escolha fica salva. Na
  primeira execução o app segue o idioma do seu Windows. O instalador também segue.

## Instalação (usar no seu PC)

1. Baixe o instalador `Developer-Black-Box_<versão>_x64-setup.exe` na aba **Releases** deste
   repositório (e o `SHA256SUMS.txt`, se quiser conferir o arquivo).
2. Rode o instalador. Ele instala **só para o seu usuário** (não pede administrador).
3. O instalador **não é assinado digitalmente**, então o Windows SmartScreen pode avisar "O Windows
   protegeu o computador". Isso é esperado num projeto pessoal sem certificado de assinatura: escolha
   "Mais informações" > "Executar assim mesmo" se você confia no arquivo. Para conferir que ele é o mesmo
   da Release, compare o hash:

```powershell
(Get-FileHash ".\Developer-Black-Box_0.1.0_x64-setup.exe" -Algorithm SHA256).Hash.ToLower()
```

Na primeira execução o app **começa pausado**: nada é gravado até você clicar em "Retomar gravação"
(ou ligar "Começar a gravar ao abrir" em Privacidade). Para desinstalar, use "Aplicativos instalados"
do Windows. Seus dados gravados **não** são apagados: use "Excluir tudo" na aba Armazenamento antes de
desinstalar, ou apague `%LOCALAPPDATA%\DeveloperBlackBox`.

## Compilando você mesmo

Requisitos: Windows 11, [Rust](https://rustup.rs) + Build Tools do Visual Studio (C++), Node 22+ e o
WebView2 (já vem no Windows 11).

```bash
npm install
npx tauri build                            # gera o instalador (alvo NSIS definido em tauri.conf.json)
# instalador: target/release/bundle/nsis/
```

Só o executável, sem instalador:

```bash
npm run build && cargo build -p bb-app --release --features tauri/custom-protocol
# target/release/developer-blackbox.exe
```

A primeira vez que você gera o instalador, o Tauri baixa as ferramentas do NSIS (do GitHub).

### Testes

```bash
cargo test --workspace   # todos os testes de Rust, incluindo tests/privacy/
npm test                 # testes dos scripts de segurança do repositório
npm run check:repo       # nenhum arquivo sensível rastreado + scanner de segredos
```

Os testes de privacidade usam **somente dados sintéticos** e cobrem, entre outros: pausa manual, retomada
que respeita o Guard, apps excluídos, autorizações temporárias, cifra em disco, exportação com nova
filtragem, recuperação após queda e ausência de bibliotecas de rede no núcleo.

### Publicar uma Release (mantenedor)

O workflow `.github/workflows/release.yml` roda os testes, gera o instalador e o anexa à Release quando
você cria uma tag (ele ainda não foi executado no GitHub; teste-o com "Run workflow" antes):

```bash
git tag v0.1.0 && git push origin v0.1.0
```

Antes de publicar, rode `npm run check:repo` e `npm run check:history` (procuram arquivos sensíveis e
segredos no repositório e em todo o histórico) e, de preferência, também um scanner dedicado como o
`gitleaks`.

## Onde ficam os dados

`%LOCALAPPDATA%\DeveloperBlackBox\`: `key.bin` (chave protegida por DPAPI), `meta.db`, `recorder\`
(segmentos cifrados) e `exports\`. Nada disso vive no repositório e tudo está no `.gitignore`.

## Como a privacidade é garantida

- **Esquema fechado:** os eventos só têm campos numéricos, PID e o **nome do executável**. Não existe
  campo de texto livre, então não há onde um segredo entrar.
- **Todo evento passa pelo Privacy Guard** antes de chegar ao gravador (o tipo do evento só pode ser
  criado pelo Guard).
- **Sem reconstrução:** o que ocorre durante uma pausa ou bloqueio nunca é gravado depois, inclusive
  falhas registradas pelo Windows nesse intervalo.
- **Cifra em repouso:** eventos e campos sensíveis do banco em AES-256-GCM, com chave protegida por
  DPAPI (ligada à sua conta do Windows). Conteúdo apagado é sobrescrito no arquivo.
- **Exportação refiltrada:** aplica de novo as regras de privacidade de agora e nunca inclui anotações.
- **Sem nuvem e sem telemetria.** O núcleo não depende de nenhuma biblioteca de rede.

## Limitações

- Uma queda de energia ou do Windows pode perder os últimos eventos ainda no cache do disco (queda só
  do processo não perde nada).
- Processos protegidos ou elevados não são vistos.
- A proteção depende da conta do Windows: quem usa a sua conta aberta consegue usar a chave.
- O arquivo de exportação **não é cifrado** (a interface avisa).
- Metadados como o número de incidentes e horários ficam no banco sem cifra (só nomes de app,
  anotações e configurações são cifrados).
- A detecção de contexto sensível é por aplicativo em primeiro plano, não por campo de senha.
- ETW não foi implementado (o log Application do Windows e os snapshots de processo cobrem falhas,
  travamentos e recursos).
- A bandeja é verificada por testes só na lógica; o resultado visual é conferido manualmente.

## Estrutura

```
crates/bb-core       eventos, máquina de estados, Privacy Guard e autorizações de teste
crates/bb-recorder   gravação cifrada em segmentos, cadeia de hashes, retenção, recuperação, DPAPI
crates/bb-collector  processos, métricas, contexto do Windows, Event Log, início com o Windows
crates/bb-store      SQLite (incidentes, anotações, configurações) com campos sensíveis cifrados
crates/bb-query      consultas de leitura e exportação com nova filtragem
crates/bb-engine     ciclo de coleta, incidentes, configurações e exportação
crates/bb-tray       ícone, textos e regras do menu da bandeja
src-tauri            aplicativo Tauri (bandeja, janela e comandos)
src                  interface React + TypeScript (src/i18n: dicionários em inglês e português)
tests/privacy        testes de privacidade e segurança (dados sintéticos)
scripts              verificações de segurança do repositório
```

## Licença

[PolyForm Noncommercial 1.0.0](LICENSE): o código é público para estudo e uso não comercial. Uso
comercial não é permitido. Não é uma licença open source no sentido da OSI. O arquivo da licença também
traz uma tradução não oficial em português; vale o texto em inglês.
