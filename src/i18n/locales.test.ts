import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import enCommon from "../locales/en/common.json";
import deCommon from "../locales/de/common.json";
import ptCommon from "../locales/pt/common.json";
import esCommon from "../locales/es/common.json";
import { toneFromMessage } from "../utils/toastTone";

const en = enCommon as Record<string, string>;
const de = deCommon as Record<string, string>;
const pt = ptCommon as Record<string, string>;
const es = esCommon as Record<string, string>;

/** Nomes dos `{{placeholder}}` de uma frase, ordenados — é o que precisa sobreviver à tradução. */
function placeholders(text: string): string[] {
  return [...text.matchAll(/\{\{\s*([^}]+?)\s*\}\}/g)].map((m) => m[1]).sort();
}

/**
 * Chaves que ficam idênticas ao inglês de propósito: jargão do Roblox, nome de
 * funcionalidade do app e sigla que a comunidade usa em inglês. Qualquer outra
 * chave igual ao inglês é tradução esquecida, e o teste abaixo reprova.
 *
 * Cada idioma completo tem a sua lista: a palavra que coincide em português
 * ("Volume", "Console") não é a mesma que coincide em espanhol.
 */
const IDENTICAL_BY_DESIGN_PT = new Set<string>([
  // Presets de launch: "place" e "VIP" não se traduzem (glossário).
  "Place {{placeId}}",
  "VIP: {{name}}",
  "WebServer",
  "Watcher",
  "online",
  "studio",
  "Multi Roblox",
  // Nome da funcionalidade na tela; traduzir "Auto Rejoin" isolado criaria um
  // segundo nome para a mesma coisa.
  "Auto Rejoin",
  "Auto Rejoin ({{count}})",
  "auto rejoin",
  // Botão dos tutoriais de tela: "Tutorial" é a mesma palavra em português.
  "Tutorial",
  "OK",
  "Nexus",
  "ID: {{id}}",
  "MultiAlt",
  "Roblox Account Manager",
  "_|WARNING:-DO-NOT-SHARE...",
  "auth ticket",
  "cookie",
  "Cookie",
  "FPS",
  // Rotulo de campo na sidebar da conta: "Volume" e a mesma palavra em pt-BR.
  "Volume",
  "ID: ********",
  "username:password",
  "Job",
  "Job ID",
  "Offline",
  "Online",
  "Ping",
  "Place",
  "Place {{id}}",
  "Place ID",
  "Script",
  "theme-preset",
  "user ID",
  "User ID",
  "user:pass",
  "VIP",
  "Popup",
  "{\"assets\":[{\"id\":12345}]}",
  "<city>, <countryCode>",
  "C:\\path\\ClientAppSettings.json",
  "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  "MB",
  "min",
  "ms",
  "Normal",
  "Roblox",
  "Universe ID",
  // Perfil de client settings por papel: o nome do recurso fica em inglês, e
  // "main"/"alt" é como a comunidade chama conta principal e conta secundária.
  "Auto Rejoin Alt",
  "Auto Rejoin Main",
  "EcoQoS",
  "Social",
  "Catppuccin",
  "Global",
  "Graphite",
  "Legacy v4 (Original)",
  "Ocean",
  "Sunset",
  "Endpoint",
  "Cooldown",
  "Status",
  "{\n  \"DFFlagTextureQualityOverrideEnabled\": true,\n  \"DFIntTextureQualityOverride\": 0\n}",
  "ABCD-EFGH",
  "alt",
  "API",
  "Beta",
  "BLOX-XXXXXXXXXXXXXXXX",
  "BloxGen",
  "Bubble",
  "Console",
  "DM Sans",
  "dump",
  "Editor",
  "Fira Code",
  "IBM Plex Mono",
  "IBM Plex Sans",
  "Info",
  "Inter",
  "Jakarta",
  "JavaScript",
  "JetBrains Mono",
  "Link",
  "LIVE",
  "Logs",
  "Manrope",
  "MIT",
  "MIT, Latte Softworks",
  "Monitor {{n}}",
  "Nexus + WebServer",
  "Noto Sans",
  "Nunito",
  "place {{placeId}}",
  "Plex",
  "Plus Jakarta Sans",
  "Poppins",
  "Pre-Hyperion",
  "Roboto",
  "Roboto Mono",
  "Rubik",
  "Scripts",
  "Soft",
  "Source Code Pro",
  "Space Grotesk",
  "Space Mono",
  "Studio",
  "Terminal",
  "UI",
  "Use ram.invoke(command, args), ram.http.request(...), ram.ws.connect/send/on(...), ram.window.snapshot(), ram.settings.get/set(), ram.modal.confirm(), ram.ui.set().",
  "version-abcdef0123456789",
  "WebSocket",
  "WhatExpsAre.Online",
  "Auth ticket",
  "PIN",
  "Presets",
  "{{n}} backups",
  "Backups",
  "{{count}} online",
  "antes da limpeza",
  "Entra de novo quando o cliente cai.",
  "Fixture do harness.",
  "Intel(R) Wi-Fi 6 AX201",
  "Realtek PCIe GbE Family Controller",
  "ws://localhost:{{port}}/Nexus",
  // Identificador do processo do Windows, igual em qualquer idioma.
  "PID {{pid}}",
]);

/** Mesma regra para o espanhol: marcas, fontes, jargão e exemplos de formato. */
const IDENTICAL_BY_DESIGN_ES = new Set<string>([
  // Presets de launch: "place" e "VIP" não se traduzem (glossário).
  "Place {{placeId}}",
  "VIP: {{name}}",
  "General",
  "WebServer",
  "Watcher",
  "auto rejoin",
  "studio",
  "Multi Roblox",
  "Auto Rejoin",
  "OK",
  "Nexus",
  // Botão dos tutoriais de tela: "Tutorial" é a mesma palavra em espanhol.
  "Tutorial",
  "Error: {{error}}",
  "ID: {{id}}",
  "MultiAlt",
  "_|WARNING:-DO-NOT-SHARE...",
  "auth ticket",
  "cookie",
  "Cookie",
  "FPS",
  "ID: ********",
  "username:password",
  "Job",
  "Job ID",
  "Ping",
  "Place",
  "Place {{id}}",
  "Place ID",
  "Script",
  "theme-preset",
  "user ID",
  "User ID",
  "user:pass",
  "VIP",
  "{\"assets\":[{\"id\":12345}]}",
  "<city>, <countryCode>",
  "C:\\path\\ClientAppSettings.json",
  "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  "MB",
  "min",
  "ms",
  "Normal",
  "Roblox",
  "Universe ID",
  "Auto Rejoin Alt",
  "Auto Rejoin Main",
  "EcoQoS",
  "Social",
  "Catppuccin",
  "Global",
  "Graphite",
  "Legacy v4 (Original)",
  "Ocean",
  "Sunset",
  "Endpoint",
  "Cooldown",
  "Error",
  "{\n  \"DFFlagTextureQualityOverrideEnabled\": true,\n  \"DFIntTextureQualityOverride\": 0\n}",
  "ABCD-EFGH",
  "alt",
  "API",
  "Beta",
  "BLOX-XXXXXXXXXXXXXXXX",
  "BloxGen",
  "Bubble",
  "DM Sans",
  "dump",
  "Editor",
  "error",
  "Fira Code",
  "IBM Plex Mono",
  "IBM Plex Sans",
  "Info",
  "Inter",
  "Jakarta",
  "JavaScript",
  "JetBrains Mono",
  "Manrope",
  "MIT",
  "MIT, Latte Softworks",
  "Monitor {{n}}",
  "Nexus + WebServer",
  "Noto Sans",
  "Nunito",
  "place {{placeId}}",
  "Plex",
  "Plus Jakarta Sans",
  "Poppins",
  "Pre-Hyperion",
  "Roboto",
  "Roboto Mono",
  "Rubik",
  "Scripts",
  "Soft",
  "Source Code Pro",
  "Space Grotesk",
  "Space Mono",
  "Studio",
  "Terminal",
  "UI",
  "version-abcdef0123456789",
  "WebSocket",
  "WhatExpsAre.Online",
  "Auth ticket",
  "PIN",
  "Intel(R) Wi-Fi 6 AX201",
  "Realtek PCIe GbE Family Controller",
  "ws://localhost:{{port}}/Nexus",
  // Identificador do processo do Windows, igual em qualquer idioma.
  "PID {{pid}}",
  "Auto Rejoin ({{count}})",
  "Roblox Account Manager",
]);

/** Idiomas completos: cobrem o inglês inteiro, cada um com a sua lista de jargão. */
const COMPLETE_CATALOGS: [string, Record<string, string>, Set<string>][] = [
  ["pt", pt, IDENTICAL_BY_DESIGN_PT],
  ["es", es, IDENTICAL_BY_DESIGN_ES],
];

describe.each(COMPLETE_CATALOGS)("catálogo %s completo", (_name, dict, identicalByDesign) => {
  it("cobre o inglês inteiro, na mesma ordem e sem chave a mais", () => {
    expect(Object.keys(dict)).toEqual(Object.keys(en));
  });

  it("não tem valor vazio nem sobra de espaço nas pontas", () => {
    const empty = Object.keys(dict).filter((k) => !dict[k].trim());
    expect(empty).toEqual([]);
    const padded = Object.keys(dict).filter((k) => dict[k] !== dict[k].trim());
    expect(padded).toEqual([]);
  });

  it("traduziu tudo que não é jargão", () => {
    const untranslated = Object.keys(dict).filter((k) => dict[k] === en[k] && !identicalByDesign.has(k));
    expect(untranslated).toEqual([]);
  });

  it("só mantém em inglês o que está na lista de jargão", () => {
    const stale = [...identicalByDesign].filter((k) => !(k in en) || dict[k] !== en[k]);
    expect(stale).toEqual([]);
  });
});

describe.each([
  ["pt", pt],
  ["es", es],
  ["de", de],
])("catálogo %s", (_name, dict) => {
  it("não inventa chave fora do inglês", () => {
    expect(Object.keys(dict).filter((k) => !(k in en))).toEqual([]);
  });

  it("preserva os placeholders de cada frase", () => {
    const broken = Object.keys(dict)
      .filter((k) => placeholders(k).join(",") !== placeholders(dict[k]).join(","))
      .map((k) => `${JSON.stringify(k)} -> ${JSON.stringify(dict[k])}`);
    expect(broken).toEqual([]);
  });
});

/**
 * Atributo JSX entre aspas **não é string literal de JS**: `attr="a\\b"` entrega
 * ao componente as duas barras, e `attr="linha1\nlinha2"` entrega o `\n` como
 * dois caracteres visíveis. O estrago é triplo, porque essas strings são chaves
 * do catálogo:
 *
 * 1. a tela mostra `HKLM\\SOFTWARE\\...` ou um JSON de uma linha com `\n` cru;
 * 2. a chave pedida (escapada) não existe no catálogo — o `en` guarda a forma
 *    desescapada — então `t()` cai no `defaultValue` e a frase sai em inglês em
 *    **todos** os idiomas;
 * 3. a tradução correspondente vira chave morta que ninguém nunca vê.
 *
 * A forma certa é `attr={"a\\b"}`: dentro de `{}` é expressão JS e o escape é
 * processado uma vez. Este teste varre o frontend para que um quarto caso não
 * entre em silêncio.
 */
const SRC_ROOT = resolve(process.cwd(), "src");

/** Atributo JSX de valor literal, numa linha: `nome="..."` ou `nome='...'`. */
const JSX_LITERAL_ATTR = /(?:^|[\s{])([A-Za-z][A-Za-z0-9]*(?:-[A-Za-z0-9]+)*)=("[^"\n]*"|'[^'\n]*')/g;

/**
 * Texto **filho** de JSX na mesma linha: `<span>C:\\caminho</span>`. Ali o escape
 * também sai cru na tela, e a varredura de atributos não o alcança.
 */
const JSX_TEXT_CHILD = />([^<>{}]*\\(?:\\|[nrt])[^<>{}]*)</g;

/** Escape que chega cru à tela: barra dupla, `\n`, `\t` ou `\r`. */
const RAW_ESCAPE = /\\\\|\\[nrt]/;

function tsxFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) out.push(...tsxFiles(full));
    else if (entry.endsWith(".tsx")) out.push(full);
  }
  return out.sort();
}

function rawEscapesInJsxAttributes(): string[] {
  const found: string[] = [];
  for (const file of tsxFiles(SRC_ROOT)) {
    const lines = readFileSync(file, "utf8").split("\n");
    lines.forEach((line, index) => {
      const where = `${relative(process.cwd(), file).replace(/\\/g, "/")}:${index + 1}`;
      for (const match of line.matchAll(JSX_LITERAL_ATTR)) {
        const literal = match[2].slice(1, -1);
        if (!RAW_ESCAPE.test(literal)) continue;
        found.push(`${where} ${match[1]}=${match[2]}`);
      }
      for (const match of line.matchAll(JSX_TEXT_CHILD)) {
        found.push(`${where} texto JSX: ${match[1].trim()}`);
      }
    });
  }
  return found;
}

describe("escape em atributo JSX", () => {
  it("nenhum atributo nem texto JSX carrega escape cru (`\\\\`, `\\n`, `\\t`, `\\r`)", () => {
    expect(rawEscapesInJsxAttributes()).toEqual([]);
  });

  it("a varredura realmente enxerga arquivos (protege contra regex morta)", () => {
    expect(tsxFiles(SRC_ROOT).length).toBeGreaterThan(50);
  });
});

/**
 * O tom do toast é deduzido do texto da mensagem (`toneFromMessage`), porque a
 * maior parte dos call sites entrega a frase já traduzida. Logo a tradução pode
 * **apagar** um tom: "Launch failed" vira "Não foi possível iniciar", e sem
 * marcador em português o erro cairia como `info`.
 *
 * A regra é não enfraquecer: se o inglês já indica desfecho, a tradução tem de
 * indicar o mesmo. O contrário é permitido — o português pega "Não foi possível"
 * onde o heurístico inglês deixa passar "Could not", e isso é melhoria, não bug.
 */
const TONE_EXCEPTIONS = new Set<string>([
  // Texto de corpo e título de passo, nunca vão a toast: o inglês só cai em
  // "success" porque a frase contém "launched"/"started".
  "All {{count}} selected accounts will join the same game that this player is currently in, launched one at a time.",
  "Every few seconds the watcher checks each Roblox client this app launched and closes the ones that match a rule below.",
  "It never reopens them, ignores clients you started outside the app, and skips the window you are using right now.",
  "Getting Started",
  // Texto de corpo da importação do TinyTask (aba Recordings): "saved" no
  // inglês não faz dele um aviso de sucesso.
  "Not saved yet: pick exactly one account to test these steps, or save to play them on several.",
  "Pick the window you recorded in and don't move or resize it before importing. Record in a window the same size as your accounts' windows (for example after Arrange in grid): clicks are saved as a position relative to the window.",
]);

describe.each([
  ["pt", pt],
  ["es", es],
  ["de", de],
])("tom do toast sobrevive à tradução (%s)", (_name, dict) => {
  it("nenhuma tradução apaga o tom que o inglês indica", () => {
    const weakened = Object.keys(dict)
      .filter((k) => !TONE_EXCEPTIONS.has(k))
      .filter((k) => {
        const source = toneFromMessage(en[k]);
        return source !== "info" && toneFromMessage(dict[k]) !== source;
      })
      .map((k) => `${JSON.stringify(k)} (${toneFromMessage(en[k])}) -> ${JSON.stringify(dict[k])} (${toneFromMessage(dict[k])})`);
    expect(weakened).toEqual([]);
  });

  it("as exceções continuam sendo texto de corpo presente no catálogo", () => {
    expect([...TONE_EXCEPTIONS].filter((k) => !(k in en))).toEqual([]);
  });
});

/**
 * O recurso se chama **Auto Rejoin** na tela. Por dentro ele continua `botting`
 * — chave do `RAMSettings.ini`, comando Tauri, evento, nome de arquivo, de
 * módulo e de função — porque renomear isso apagaria a configuração de quem já
 * usa o app. Essa fronteira é fácil de furar sem querer: basta um rótulo novo
 * copiado de um bloco antigo.
 *
 * A varredura abaixo é o que impede o nome antigo de voltar à tela. Ela olha o
 * catálogo inteiro (todo texto de UI passa por ali) e, no fonte, as strings
 * literais e o texto JSX de arquivo que não é de teste — é onde mora um rótulo
 * que ainda não chegou ao catálogo. Comentário fica de fora de propósito: ali o
 * nome interno é o nome certo.
 */
const NOME_ANTIGO = /\bBotting\b/;

/** Arquivos de fonte que carregam texto de tela (teste e catálogo ficam fora). */
function uiSourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === "locales") continue;
      out.push(...uiSourceFiles(full));
      continue;
    }
    if (!/\.(ts|tsx)$/.test(entry) || /\.test\.(ts|tsx)$/.test(entry)) continue;
    out.push(full);
  }
  return out.sort();
}

/** String literal de JS/TS: é o que chega a `t()`, a um `label=` ou a um toast. */
const STRING_LITERAL = /"((?:\\.|[^"\\])*)"|'((?:\\.|[^'\\])*)'|`((?:\\.|[^`\\])*)`/g;

/** Texto filho de JSX na mesma linha: `label={<>Auto Rejoin<Badge/></>}`. */
const JSX_TEXT = />([^<>{}]*)</g;

function uiTextsWithOldName(): string[] {
  const found: string[] = [];
  for (const file of uiSourceFiles(SRC_ROOT)) {
    const where = relative(process.cwd(), file).replace(/\\/g, "/");
    readFileSync(file, "utf8")
      .split("\n")
      .forEach((line, index) => {
        const candidates: string[] = [];
        for (const m of line.matchAll(STRING_LITERAL)) candidates.push(m[1] ?? m[2] ?? m[3] ?? "");
        for (const m of line.matchAll(JSX_TEXT)) candidates.push(m[1]);
        for (const text of candidates) {
          if (NOME_ANTIGO.test(text)) found.push(`${where}:${index + 1} ${JSON.stringify(text)}`);
        }
      });
  }
  return found;
}

describe("o nome na tela é Auto Rejoin", () => {
  it.each([
    ["en", en],
    ["pt", pt],
    ["es", es],
    ["de", de],
  ])("nenhuma chave nem tradução do catálogo %s diz o nome antigo", (_name, dict) => {
    const leaked = Object.keys(dict)
      .filter((k) => /botting/i.test(k) || /botting/i.test(dict[k]))
      .map((k) => `${JSON.stringify(k)} -> ${JSON.stringify(dict[k])}`);
    expect(leaked).toEqual([]);
  });

  it("nenhuma string nem texto JSX do fonte diz o nome antigo", () => {
    expect(uiTextsWithOldName()).toEqual([]);
  });

  it("a varredura realmente enxerga texto de tela (protege contra regex morta)", () => {
    expect(uiSourceFiles(SRC_ROOT).length).toBeGreaterThan(50);
    // Este arquivo é de teste, então fica fora da própria varredura: dá para
    // citar o nome antigo aqui sem a varredura se autodenunciar.
    expect(NOME_ANTIGO.test("Start Botting Mode")).toBe(true);
    // Nome interno não é texto de tela: a varredura não pode reprovar por ele.
    expect(NOME_ANTIGO.test("BottingDraftPlaceId")).toBe(false);
    expect(NOME_ANTIGO.test("supportsBotting")).toBe(false);
  });
});
