/**
 * Anonimização do resumo do "Reportar problema" (ideia 28).
 *
 * O texto passa por aqui **antes** de aparecer na tela de prévia, e é a prévia
 * que vai para a área de transferência — então o que a pessoa vê é exatamente
 * o que sai. Some: cookie, token, senha, nome/alias de conta, ID de usuário,
 * caminho com o nome do usuário do Windows, Job ID, código de servidor
 * privado, e-mail e IP.
 *
 * A ordem importa: segredos conhecidos e cookies primeiro (eles contêm
 * qualquer coisa, inclusive o que parece nome ou número), depois nomes e IDs,
 * depois os padrões genéricos.
 */

export interface AnonymizeContext {
  /** Nomes de usuário, aliases e afins das contas salvas. */
  accountNames?: string[];
  /** IDs de usuário das contas salvas. */
  userIds?: number[];
  /** Valores que nunca podem aparecer (cookies e senhas das contas). */
  secrets?: string[];
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

const COOKIE_WARNING = /_\|WARNING:[^|]*\|_[^\s"';,&]*/gi;
const ROBLOSECURITY = /(\.ROBLOSECURITY\s*[=:]\s*)("?)[^\s"';,&]+/gi;
/** `chave=valor` / `chave: valor` cujo valor é segredo. */
const SECRET_FIELDS =
  /\b(cookie|set-cookie|authorization|x-csrf-token|csrf|rbx-authentication-ticket|auth[-_ ]?ticket|ticket|access[-_ ]?token|refresh[-_ ]?token|token|private[-_ ]?key|privatekey|password|passwd|pwd|api[-_ ]?key|secret)(\s*["']?\s*[:=]\s*)("?)([^\s"&;,]+)/gi;
/** Sequência opaca longa (token, hash de sessão, chave em base64). */
const LONG_TOKEN = /[A-Za-z0-9+/_-]{40,}={0,2}/g;
const WINDOWS_USER_PATH = /\b([A-Za-z]:[\\/]+(?:Users|Documents and Settings)[\\/]+)([^\\/\r\n"'<>|:*?]+)/gi;
const UNIX_USER_PATH = /(\/(?:Users|home)\/)([^/\s"'<>]+)/g;
const UUID = /\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/gi;
const VIP_CODE = /\bvip:[^\s"',;]+/gi;
const PRIVATE_CODES =
  /\b(privateServerLinkCode|linkCode|accessCode|privateServerId|reservedServerAccessCode|launchData)(\s*[=:]\s*)([^\s"&;,]+)/gi;
const SHARE_LINK = /(roblox\.com\/share\?)[^\s"']+/gi;
const EMAIL = /\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b/g;
const IPV4 = /\b(?:(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\.){3}(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(?::\d{1,5})?\b/g;
const IPV6 = /(?<![0-9a-z:])(?:[0-9a-f]{0,4}:){2,7}[0-9a-f]{0,4}(?:%\w+)?(?![0-9a-z:])/gi;
/** "user 123", "userId=123", "UserID: 123", "uid 123" — com ID de usuário. */
const USER_ID_MENTION = /\b(user\s*id|userid|user_id|uid|user)(\s*[#:=]?\s*)(\d{3,})\b/gi;

/** "a::b" ou "std::x" não são endereço: exige 4+ dígitos hex e grupos suficientes. */
function looksLikeIpv6(match: string): boolean {
  const colons = (match.match(/:/g) ?? []).length;
  const hexDigits = (match.match(/[0-9a-f]/gi) ?? []).length;
  return hexDigits >= 4 && (match.includes("::") ? colons >= 3 : colons >= 5);
}

export function anonymize(text: string, context: AnonymizeContext = {}): string {
  let out = text;

  // 1. Segredos conhecidos, pelo valor exato.
  const secrets = [...new Set((context.secrets ?? []).map((s) => s.trim()).filter((s) => s.length >= 4))].sort(
    (a, b) => b.length - a.length
  );
  for (const secret of secrets) {
    out = out.split(secret).join("[secret]");
  }

  // 2. Cookie do Roblox, com ou sem o nome na frente.
  out = out.replace(COOKIE_WARNING, "[cookie]");
  out = out.replace(ROBLOSECURITY, "$1$2[cookie]");

  // 3. Campos de segredo e tokens longos.
  out = out.replace(SECRET_FIELDS, (_m, key: string, sep: string, quote: string) => `${key}${sep}${quote}[secret]`);
  out = out.replace(LONG_TOKEN, "[token]");

  // 4. Nomes das contas (mais longos primeiro, para "ana_alt" não virar
  //    "[account]_alt" por causa de "ana").
  const names = [...new Set((context.accountNames ?? []).map((n) => n.trim()).filter((n) => n.length >= 3))].sort(
    (a, b) => b.length - a.length
  );
  for (const name of names) {
    const pattern = new RegExp(`(?<![A-Za-z0-9_])${escapeRegExp(name)}(?![A-Za-z0-9_])`, "gi");
    out = out.replace(pattern, "[account]");
  }

  // 5. IDs de usuário conhecidos e menções genéricas a ID de usuário.
  const ids = [...new Set((context.userIds ?? []).filter((id) => Number.isFinite(id) && id > 0))];
  for (const id of ids) {
    out = out.replace(new RegExp(`(?<!\\d)${id}(?!\\d)`, "g"), "[user-id]");
  }
  out = out.replace(USER_ID_MENTION, "$1$2[user-id]");

  // 6. Caminhos com o nome do usuário do Windows (ou do Mac/Linux).
  out = out.replace(WINDOWS_USER_PATH, "$1<user>");
  out = out.replace(UNIX_USER_PATH, "$1<user>");

  // 7. Servidores: Job ID, VIP, códigos de servidor privado e share links.
  out = out.replace(UUID, "[job-id]");
  out = out.replace(VIP_CODE, "vip:[private]");
  out = out.replace(PRIVATE_CODES, "$1$2[private]");
  out = out.replace(SHARE_LINK, "$1[private]");

  // 8. E-mail e IP.
  out = out.replace(EMAIL, "[email]");
  out = out.replace(IPV4, "[ip]");
  // Hora "12:34:56" tem o mesmo formato: só conta com "::" ou com 6+ grupos.
  out = out.replace(IPV6, (match) =>
    looksLikeIpv6(match) ? "[ip]" : match
  );

  return out;
}
