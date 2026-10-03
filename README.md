# Developer Black Box

**English** | [Português (Brasil)](#português-brasil)

A local "black box" for **Windows 11**: it records technical process events (start, exit, CPU,
memory, crashes and hangs), preserves incident evidence and helps you investigate software problems.
**Privacy first:** everything stays on your computer, encrypted, with no cloud, and the app never
captures passwords, typed text, page content, window titles, URLs, file paths or command lines.

**[Download the latest installer](https://github.com/GabrielVianaNunes/Developer-Black-Box/releases/latest)** · Windows 11 · free for noncommercial use

![Developer Black Box overview](assets/readme-images/overview.png)

*Screenshots in this README show a fresh install with made-up data (example program names only). Nothing in them was recorded from a real computer.*

## What it does

- **Tray icon** (a black cube with a light beside it): green only when recording is actually active;
  red when paused, suspended by privacy rules or failing. The tooltip and the window explain why.
- **Pause and resume** from the tray, even with the window closed. Resuming never bypasses the
  Privacy Guard.
- **Privacy Guard:** suspends recording when a browser or password manager is in the foreground, the
  session is locked or the detector is unavailable. The absence of a signal is never treated as "safe".
- **Incidents:** manual capture, sustained CPU, high memory, crashes and hangs (Windows Event Log),
  with evidence from the window before and after.
- **System health events:** unexpected shutdowns, blue screens (stop code), hardware errors (WHEA), display driver
  resets, disk and NTFS errors, services that stopped unexpectedly and failed Windows Updates, from a **fixed list of
  Event Log IDs**. Only the category, the event ID and one number (such as the stop code) are stored, never the message
  text. They keep being recorded while a privacy block is active (they do not depend on the app in front), but a manual
  pause always wins.
- **Power and battery:** plugged in or on battery and the charge percentage (only meaningful changes), plus sleep and resume from the
  Windows log. A computer without a battery shows this source as unavailable. No location and no network information. Battery wear is
  **not** recorded: it was not shown to be readable without administrator rights.
- **System inventory changes:** the BIOS version and date, UEFI or legacy firmware, Secure Boot, the Windows build and devices with a
  driver problem are read without administrator rights, and **only changes** are recorded (`previous -> new`, numbers only; never serial
  numbers, UUIDs, computer or device names, or the machine model).
- **Dashboard:** overview, activity with filters, processes, incidents (timeline, notes, export),
  privacy, storage and integrity verification.
- **Rules in effect at a glance:** the Overview lists the protected applications and each exclusion rule, with how many times
  each rule left something out (only a number, kept in memory). Protected pauses everything; an exclusion leaves out just that program.
- **Exclusion rules by event type:** leave a program out entirely, or choose what is still recorded for it
  (start and end, CPU and memory, crashes and hangs). By default nothing is. Programs are picked from a
  search of the programs on your PC (including Start Menu shortcuts) or a file picker, so there is nothing
  to mistype.
- **Guide and what's new:** a short tour on first launch and a summary after updates, built into the app,
  skippable at any time and reopenable with the **?** button. The examples are inert and use made-up data.
- **Test mode:** a temporary, revocable authorization to collect technical data from a browser, so you
  can test a web application of your own. Page content, forms and requests are never recorded.
- **Open with Windows** (optional): starts hidden in the tray and stays paused.
- **English and Portuguese (Brazil):** every screen, the tray menu and the messages are available in
  both languages. Switch at any time with the language selector at the top of the window; the choice is
  remembered. On first run the app follows your Windows language. The installer also follows it.

## Installation (use it on your PC)

![The first-run guide](assets/readme-images/guide-light.png)

1. Download the installer `Developer-Black-Box_<version>_x64-setup.exe` from the
   [latest Release](https://github.com/GabrielVianaNunes/Developer-Black-Box/releases/latest) (and `SHA256SUMS.txt` if you want to verify the file).
2. Run the installer. It installs **for your user only** (no administrator rights needed).
3. **Keep the suggested folder** (`%LOCALAPPDATA%\Programs\Developer Black Box`). It is the default and it is
   the one known to work well, including the program icon. You may pick another folder of your own (for example
   `C:\Users\<you>\Apps\Developer Black Box`), but the installer refuses folders that are known not to work:
   Program Files (it needs administrator rights) and folders directly inside `AppData\Local` or `AppData\Roaming`
   (Windows showed a generic icon there on a test PC). Updates keep using the folder you installed into.
4. The installer is **not digitally signed with a Windows code-signing certificate**, so Windows SmartScreen
   may warn "Windows protected your PC". That is expected for a personal project: choose "More info"
   > "Run anyway" if you trust the file. To check that it is the same file as the Release, compare
   the hash (replace the file name with the one you downloaded):

```powershell
(Get-FileHash ".\Developer-Black-Box_<version>_x64-setup.exe" -Algorithm SHA256).Hash.ToLower()
```

**Old icon after updating?** Windows keeps program icons in a cache. If, after updating from an older version, the taskbar button or the shortcuts still show the old icon (the cube with a gray dot beside it), restart Windows first. If it persists, rebuild the icon cache (nothing is lost; Windows recreates it; the taskbar flashes for a few seconds). In PowerShell:

```powershell
taskkill /f /im explorer.exe
Remove-Item "$env:LOCALAPPDATA\Microsoft\Windows\Explorer\iconcache*" -Force
Start-Process explorer.exe
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

Releases follow Git Flow and a documented procedure: see [RELEASING.md](RELEASING.md). Before publishing, run `npm run check:repo` and `npm run check:history` (they look for sensitive files
and secrets in the repository and in its whole history) and, preferably, a dedicated scanner such as
`gitleaks`.

## Where the data lives

`%LOCALAPPDATA%\DeveloperBlackBox\`: `key.bin` (DPAPI-protected key), `meta.db`, `recorder\` (encrypted
segments) and `exports\`. None of it lives in the repository and all of it is in `.gitignore`.

## How privacy is guaranteed

![Privacy tab with example exclusion rules](assets/readme-images/privacy-rules.png)

- **Closed schema:** events only have numeric fields, the PID and the **executable name**. There is no
  free-text field, so there is nowhere for a secret to go.
- **Every event goes through the Privacy Guard** before reaching the recorder (the event type can only
  be created by the Guard).
- **No reconstruction:** what happens during a pause or block is never recorded afterwards, including
  crashes logged by Windows in that interval. The only exception is system health events: if the app was
  **closed** (not paused) and the previous run ended while recording, the Windows log of that closed interval (up to
  7 days) is read once at the next start, so an unexpected shutdown logged on the following boot is not lost.
- **Encryption at rest:** events and the sensitive database fields use AES-256-GCM, with a key protected
  by DPAPI (tied to your Windows account). Deleted content is overwritten in the file.
- **Re-filtered export:** it applies today's privacy rules again and never includes notes.
- **No cloud and no telemetry.** The core does not depend on any network library.
  The only network access is the optional update feature, off by default: one HTTPS request to the GitHub Releases
  of this project (app name and version only) tells you a newer version exists. Nothing is downloaded until you
  click "Download update"; the installer is then checked (SHA-256 and an Ed25519 signature bound to the version)
  and discarded if it does not verify, and it is only run when you click "Install and restart". It uses Windows'
  own HTTPS stack (no third-party network library) and a test fails if any other code opens a connection.

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
crates/bb-update     optional update check (the only network code, via Windows WinHTTP)
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

**[Baixar o instalador mais recente](https://github.com/GabrielVianaNunes/Developer-Black-Box/releases/latest)** · Windows 11 · gratuito para uso não comercial

![Visão geral do Developer Black Box](assets/readme-images/overview.png)

*As imagens deste README mostram uma instalação nova com dados inventados (só nomes de programas de exemplo). Nada nelas foi gravado de um computador real.*

## O que ele faz

- **Ícone na bandeja** (um cubo preto com uma luz ao lado): verde só quando a gravação está de fato
  ativa; vermelho quando pausada, suspensa por privacidade ou com falha. O tooltip e a janela explicam
  o motivo.
- **Pausa e retomada** pela bandeja, mesmo com a janela fechada. A retomada nunca ignora o Privacy Guard.
- **Privacy Guard:** suspende a gravação com navegadores e gerenciadores de senha em primeiro plano,
  sessão bloqueada ou detector indisponível. Ausência de sinal nunca é tratada como "seguro".
- **Incidentes:** captura manual, CPU sustentada, memória alta, falhas e travamentos (Event Log do
  Windows), com evidências da janela anterior e posterior.
- **Eventos de saúde do sistema:** desligamento inesperado, tela azul (código de parada), erros de hardware (WHEA),
  reinício do driver de vídeo, erros de disco e NTFS, serviços que encerraram sem querer e falhas de atualização do
  Windows, de uma **lista fixa de IDs do Event Log**. Só a categoria, o ID do evento e um número (como o código de
  parada) são gravados, nunca o texto da mensagem. Continuam sendo gravados durante um bloqueio de privacidade (não
  dependem do app em primeiro plano), mas a pausa manual sempre vence.
- **Energia e bateria:** na tomada ou na bateria e a porcentagem de carga (só variações relevantes), mais suspensão e retomada vindas do
  log do Windows. Computador sem bateria mostra esta fonte como indisponível. Sem localização e sem informação de rede. O desgaste da
  bateria **não** é gravado: não ficou provado que dê para ler sem administrador.
- **Mudanças no inventário do sistema:** versão e data da BIOS, firmware UEFI ou legado, Secure Boot, build do Windows e dispositivos
  com problema de driver são lidos sem administrador, e **só as mudanças** são gravadas (`anterior -> novo`, só números; nunca número
  de série, UUID, nome do computador ou dos dispositivos, nem o modelo da máquina).
- **Painel:** visão geral, atividade com filtros, processos, incidentes (linha do tempo, anotações,
  exportação), privacidade, armazenamento e verificação de integridade.
- **Regras em vigor num relance:** a Visão Geral lista os aplicativos protegidos e cada regra de exclusão, com quantas vezes
  cada uma deixou algo de fora (só um número, guardado na memória). Protegido pausa tudo; exclusão deixa de fora só aquele programa.
- **Regras de exclusão por tipo de evento:** deixe um programa totalmente de fora ou escolha o que ainda é
  gravado dele (início e fim, CPU e memória, falhas e travamentos). Por padrão, nada. Os programas são
  escolhidos numa busca entre os programas do seu PC (inclusive atalhos do Menu Iniciar) ou num seletor de
  arquivo, então não há o que digitar errado.
- **Guia e novidades:** um tour curto na primeira abertura e um resumo depois das atualizações, dentro do
  app, que pode ser pulado a qualquer momento e reaberto pelo botão **?**. Os exemplos são inertes e usam
  dados inventados.
- **Modo de teste:** autorização temporária e revogável da coleta técnica de um navegador, para testar
  uma aplicação web sua. Nunca há conteúdo de páginas, formulários ou requisições.
- **Abrir com o Windows** (opcional): abre escondido na bandeja e continua pausado.
- **Inglês e português (Brasil):** todas as telas, o menu da bandeja e as mensagens existem nos dois
  idiomas. Troque a qualquer momento no seletor de idioma no topo da janela; a escolha fica salva. Na
  primeira execução o app segue o idioma do seu Windows. O instalador também segue.

## Instalação (usar no seu PC)

![O guia da primeira abertura](assets/readme-images/guide-light.png)

1. Baixe o instalador `Developer-Black-Box_<versão>_x64-setup.exe` na
   [Release mais recente](https://github.com/GabrielVianaNunes/Developer-Black-Box/releases/latest) (e o `SHA256SUMS.txt`, se quiser conferir o arquivo).
2. Rode o instalador. Ele instala **só para o seu usuário** (não pede administrador).
3. **Mantenha a pasta sugerida** (`%LOCALAPPDATA%\Programs\Developer Black Box`). Ela é o padrão e é a que
   sabemos que funciona bem, inclusive com o ícone do programa. Você pode escolher outra pasta sua (por exemplo
   `C:\Users\<você>\Apps\Developer Black Box`), mas o instalador recusa pastas que sabemos que não funcionam:
   Arquivos de Programas (exige administrador) e pastas direto dentro de `AppData\Local` ou `AppData\Roaming`
   (o Windows mostrou um ícone genérico ali num PC de teste). As atualizações continuam usando a pasta em que
   você instalou.
4. O instalador **não é assinado com um certificado de assinatura de código do Windows**, então o SmartScreen
   pode avisar "O Windows protegeu o computador". Isso é esperado num projeto pessoal: escolha
   "Mais informações" > "Executar assim mesmo" se você confia no arquivo. Para conferir que ele é o mesmo
   da Release, compare o hash (troque o nome pelo do arquivo que você baixou):

```powershell
(Get-FileHash ".\Developer-Black-Box_<versão>_x64-setup.exe" -Algorithm SHA256).Hash.ToLower()
```

**Ícone antigo depois de atualizar?** O Windows guarda os ícones dos programas em cache. Se, ao atualizar de uma versão mais antiga, o botão da barra de tarefas ou os atalhos continuarem com o ícone velho (o cubo com uma bolinha cinza ao lado), reinicie o Windows primeiro. Se persistir, refaça o cache de ícones (nada é perdido; o Windows o recria; a barra de tarefas pisca por alguns segundos). No PowerShell:

```powershell
taskkill /f /im explorer.exe
Remove-Item "$env:LOCALAPPDATA\Microsoft\Windows\Explorer\iconcache*" -Force
Start-Process explorer.exe
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

As releases seguem o Git Flow e um procedimento documentado: veja [RELEASING.md](RELEASING.md). Antes de publicar, rode `npm run check:repo` e `npm run check:history` (procuram arquivos sensíveis e
segredos no repositório e em todo o histórico) e, de preferência, também um scanner dedicado como o
`gitleaks`.

## Onde ficam os dados

`%LOCALAPPDATA%\DeveloperBlackBox\`: `key.bin` (chave protegida por DPAPI), `meta.db`, `recorder\`
(segmentos cifrados) e `exports\`. Nada disso vive no repositório e tudo está no `.gitignore`.

## Como a privacidade é garantida

![Aba Privacidade com regras de exclusão de exemplo](assets/readme-images/privacy-rules.png)

- **Esquema fechado:** os eventos só têm campos numéricos, PID e o **nome do executável**. Não existe
  campo de texto livre, então não há onde um segredo entrar.
- **Todo evento passa pelo Privacy Guard** antes de chegar ao gravador (o tipo do evento só pode ser
  criado pelo Guard).
- **Sem reconstrução:** o que ocorre durante uma pausa ou bloqueio nunca é gravado depois, inclusive
  falhas registradas pelo Windows nesse intervalo. A única exceção são os eventos de saúde do sistema: se o app
  esteve **fechado** (não pausado) e a execução anterior terminou gravando, o log do Windows desse intervalo fechado
  (até 7 dias) é lido uma vez no início seguinte, para não perder um desligamento inesperado registrado no boot.
- **Cifra em repouso:** eventos e campos sensíveis do banco em AES-256-GCM, com chave protegida por
  DPAPI (ligada à sua conta do Windows). Conteúdo apagado é sobrescrito no arquivo.
- **Exportação refiltrada:** aplica de novo as regras de privacidade de agora e nunca inclui anotações.
- **Sem nuvem e sem telemetria.** O núcleo não depende de nenhuma biblioteca de rede.
  O único acesso à rede é o recurso opcional de atualização, desligado por padrão: uma requisição HTTPS às Releases
  deste projeto no GitHub (só o nome e a versão do app) avisa que existe versão nova. Nada é baixado até você clicar
  em "Baixar atualização"; o instalador é então conferido (SHA-256 e assinatura Ed25519 amarrada à versão) e
  descartado se não conferir, e só roda quando você clica em "Instalar e reiniciar". Usa o próprio HTTPS do Windows
  (nenhuma biblioteca de rede de terceiros) e um teste falha se qualquer outro código abrir uma conexão.

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
crates/bb-update     verificação de atualização opcional (único código de rede, via WinHTTP do Windows)
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
