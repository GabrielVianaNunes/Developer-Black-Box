# Developer Black Box

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

## Instalação (usar no seu PC)

1. Baixe o instalador `Developer Black Box_<versão>_x64-setup.exe` na aba **Releases** deste
   repositório (e o `SHA256SUMS.txt`, se quiser conferir o arquivo).
2. Rode o instalador. Ele instala **só para o seu usuário** (não pede administrador).
3. O instalador **não é assinado digitalmente**, então o Windows SmartScreen pode avisar "O Windows
   protegeu o computador". Isso é esperado num projeto pessoal sem certificado de assinatura: escolha
   "Mais informações" > "Executar assim mesmo" se você confia no arquivo. Para conferir que ele é o mesmo
   da Release, compare o hash:

```powershell
(Get-FileHash ".\Developer Black Box_0.1.0_x64-setup.exe" -Algorithm SHA256).Hash.ToLower()
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
src                  interface React + TypeScript
tests/privacy        testes de privacidade e segurança (dados sintéticos)
scripts              verificações de segurança do repositório
```

## Licença

[PolyForm Noncommercial 1.0.0](LICENSE): o código é público para estudo e uso não comercial. Uso
comercial não é permitido. Não é uma licença open source no sentido da OSI.
